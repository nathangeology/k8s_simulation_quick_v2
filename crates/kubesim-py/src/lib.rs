//! KubeSim Python — PyO3 bindings for single run and batch execution.

use pyo3::prelude::*;
use pyo3::exceptions::PyValueError;
use rayon::prelude::*;

use kubesim_core::{
    AffinityType, ClusterState, DeletionCostStrategy, LabelSet, Node, NodeConditions, NodeId, NodeLifecycle, OwnerId, Pod, PodId, PodPhase,
    PodTemplate, QoSClass, ReplicaSet, ResizePolicy, Resources, SchedulingConstraints, SimTime, WhenUnsatisfiable,
};
use kubesim_ec2::Catalog;
use kubesim_engine::{DaemonSetHandler, DaemonSetSpec, DeletionCostController, Engine, Event as EngineEvent, PodSpec, ReplicaSetController, TimeMode};
use kubesim_karpenter::{
    ConsolidationHandler, ConsolidationPolicy, DrainHandler, NodePool, NodePoolLimits,
    ProvisioningHandler, SpotInterruptionHandler,
    KarpenterVersion, VersionProfile,
};
use kubesim_metrics::{MetricsCollector, MetricsConfig as RustMetricsConfig};
use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy};
use kubesim_workload::{
    load_scenario_from_str, load_scenario_from_str_seeded, ScenarioFile, Variant,
    Event as WorkloadEvent, TimeMode as ScenarioTimeMode,
    generate_random_scenario, RandomScenarioConfig,
    RangeU32, InstanceWeight, ArchetypeWeights,
    SchedulingStrategy,
};

use std::path::Path;
use rand::SeedableRng;

// ── Helpers ─────────────────────────────────────────────────────

fn scenario_time_to_engine(t: ScenarioTimeMode) -> TimeMode {
    match t {
        ScenarioTimeMode::Logical => TimeMode::Logical,
        ScenarioTimeMode::WallClock => TimeMode::WallClock,
    }
}

fn parse_time_mode(s: &str) -> PyResult<TimeMode> {
    match s {
        "logical" => Ok(TimeMode::Logical),
        "wall_clock" | "wallclock" => Ok(TimeMode::WallClock),
        _ => Err(PyValueError::new_err(format!(
            "invalid time_mode: {s:?}, expected 'logical' or 'wall_clock'"
        ))),
    }
}

fn scoring_from_workload(s: kubesim_workload::ScoringStrategy) -> ScoringStrategy {
    match s {
        kubesim_workload::ScoringStrategy::MostAllocated => ScoringStrategy::MostAllocated,
        kubesim_workload::ScoringStrategy::LeastAllocated => ScoringStrategy::LeastAllocated,
    }
}

fn resolve_pool_name(pools: &[kubesim_workload::NodePoolDef], pool_index: u32) -> String {
    let idx = pool_index as usize;
    pools.get(idx)
        .and_then(|p| p.name.clone())
        .unwrap_or_else(|| if pools.len() == 1 { "default".into() } else { format!("pool-{}", idx) })
}

fn nodepool_from_def(pool_def: &kubesim_workload::NodePoolDef, pool_name: String, variant_budget: Option<&kubesim_workload::DisruptionBudgetDef>) -> NodePool {
    let (pct, count) = match variant_budget.or(pool_def.disruption_budget.as_ref()) {
        Some(db) => (db.max_percent, db.max_count),
        None => (10, None),
    };
    NodePool {
        name: pool_name,
        instance_types: pool_def.instance_types.clone(),
        limits: NodePoolLimits {
            max_nodes: Some(pool_def.max_nodes),
            max_cpu_millis: None,
            max_memory_bytes: None,
        },
        labels: pool_def.labels.clone(),
        taints: pool_def.taints.clone(),
        max_disrupted_pct: pct,
        max_disrupted_count: count,
        weight: pool_def.weight,
        do_not_disrupt: pool_def.do_not_disrupt,
    }
}

fn parse_karpenter_version(s: &str) -> Option<KarpenterVersion> {
    match s {
        "v0.35" | "v0_35" | "0.35" => Some(KarpenterVersion::V0_35),
        "v1" | "v1.x" | "v1.0" | "1" => Some(KarpenterVersion::V1),
        _ => None,
    }
}

fn map_consolidation_policy(p: kubesim_workload::ConsolidationPolicy) -> ConsolidationPolicy {
    match p {
        kubesim_workload::ConsolidationPolicy::WhenEmpty => ConsolidationPolicy::WhenEmpty,
        kubesim_workload::ConsolidationPolicy::WhenUnderutilized => ConsolidationPolicy::WhenUnderutilized,
        kubesim_workload::ConsolidationPolicy::WhenEmptyOrUnderutilized => ConsolidationPolicy::WhenUnderutilized,
        kubesim_workload::ConsolidationPolicy::WhenCostJustifiesDisruption => ConsolidationPolicy::WhenCostJustifiesDisruption,
    }
}

/// Compute the system overhead Resources from scenario cluster config.
fn compute_overhead(cluster: &kubesim_workload::ClusterConfig) -> Resources {
    let (cpu, mem) = match &cluster.system_overhead {
        Some(oh) => (oh.cpu_millis(), oh.memory_bytes()),
        None => (0, 0),
    };
    Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 }
}

/// Build a DaemonSetHandler from scenario config.
/// None → default (logging_daemonset), Some(empty) → no daemonsets, Some(list) → custom.
fn daemonset_handler_from_scenario(defs: &Option<Vec<kubesim_workload::DaemonSetDef>>) -> DaemonSetHandler {
    match defs {
        None => DaemonSetHandler::with_defaults(),
        Some(list) => DaemonSetHandler::new(
            list.iter().map(|d| DaemonSetSpec {
                name: d.name.clone(),
                cpu_millis: d.cpu_millis(),
                memory_bytes: d.memory_bytes(),
            }).collect(),
        ),
    }
}

fn instance_to_node_with_pct(catalog: &Catalog, instance_type: &str, overhead: &Resources, daemonset_pct: u32) -> Node {
    let it_spec = catalog.get(instance_type);
    let (cpu, mem, gpu, cost, vcpu) = it_spec
        .map(|it| (
            it.vcpu as u64 * 1000,
            it.memory_gib as u64 * 1024 * 1024 * 1024,
            it.gpu_count,
            it.on_demand_price_per_hour,
            it.vcpu,
        ))
        .unwrap_or((4000, 16 * 1024 * 1024 * 1024, 0, 0.192, 4));

    // Use EKS table overhead when no explicit flat overhead is set, otherwise use flat
    let (oh_cpu, oh_mem) = if overhead.cpu_millis == 0 && overhead.memory_bytes == 0 {
        kubesim_ec2::eks_overhead(vcpu)
    } else {
        (overhead.cpu_millis, overhead.memory_bytes)
    };
    let mut alloc_cpu = cpu.saturating_sub(oh_cpu);
    let mut alloc_mem = mem.saturating_sub(oh_mem);
    if daemonset_pct > 0 {
        alloc_cpu = alloc_cpu.saturating_sub(cpu * daemonset_pct as u64 / 100);
        alloc_mem = alloc_mem.saturating_sub(mem * daemonset_pct as u64 / 100);
    }

    Node {
        instance_type: instance_type.to_string(),
        allocatable: Resources {
            cpu_millis: alloc_cpu,
            memory_bytes: alloc_mem,
            gpu,
            ephemeral_bytes: 0,
        },
        allocated: Resources::default(),
        pods: Default::default(),
        conditions: NodeConditions { ready: true, ..Default::default() },
        labels: LabelSet::default(),
        taints: Default::default(),
        cost_per_hour: cost,
        lifecycle: NodeLifecycle::OnDemand,
        cordoned: false,
        created_at: SimTime(0),
        pool_name: String::new(),
        do_not_disrupt: false,
    }
}

// ── Domain budget for ReverseSchedule ───────────────────────────

/// Tracks per-owner, per-topology-key pod counts across domains for fast constraint checking.
struct DomainBudget {
    /// (owner_id, topology_key) -> (max_per_domain, domain_value -> current_count)
    budgets: std::collections::HashMap<(OwnerId, String), (u32, std::collections::HashMap<String, u32>)>,
}

impl DomainBudget {
    fn new() -> Self { Self { budgets: std::collections::HashMap::new() } }

    fn register(&mut self, owner: OwnerId, topology_key: String, max_skew: u32) {
        self.budgets.entry((owner, topology_key)).or_insert((max_skew, std::collections::HashMap::new()));
    }

    fn has_room(&self, owner: OwnerId, topology_key: &str, domain_value: &str) -> bool {
        match self.budgets.get(&(owner, topology_key.to_string())) {
            Some((max_skew, counts)) => {
                let current = counts.get(domain_value).copied().unwrap_or(0);
                // In ReverseSchedule, nodes arrive one at a time. Using global
                // skew (current+1 - min) allows overpacking when few domains
                // exist. Instead, enforce a hard cap: at most max_skew pods per
                // domain. This is equivalent to the skew constraint when all
                // domains are evenly filled (the steady-state goal).
                current < *max_skew
            }
            None => true,
        }
    }

    fn record_placement(&mut self, owner: OwnerId, topology_key: &str, domain_value: &str) {
        if let Some((_, counts)) = self.budgets.get_mut(&(owner, topology_key.to_string())) {
            *counts.entry(domain_value.to_string()).or_insert(0) += 1;
        }
    }

    fn register_domain(&mut self, topology_key: &str, domain_value: &str) {
        for ((_, tk), (_, counts)) in &mut self.budgets {
            if tk == topology_key {
                counts.entry(domain_value.to_string()).or_insert(0);
            }
        }
    }
}

// ── Capacity-waiting queue for ReverseSchedule O(1) NodeReady ────

/// Pods waiting for new node capacity, grouped by constraint class.
/// On NodeReady, pop from the appropriate queue instead of scanning all pending.
struct CapacityWaitQueue {
    /// Pods with spread/anti-affinity constraints, keyed by owner.
    constrained: std::collections::HashMap<OwnerId, std::collections::VecDeque<PodId>>,
    /// Pods with no placement constraints (can go on any node).
    unconstrained: std::collections::VecDeque<PodId>,
    /// Track which pods are in the queue for O(1) staleness checks.
    in_queue: std::collections::HashSet<PodId>,
}

impl CapacityWaitQueue {
    fn new() -> Self {
        Self {
            constrained: std::collections::HashMap::new(),
            unconstrained: std::collections::VecDeque::new(),
            in_queue: std::collections::HashSet::new(),
        }
    }

    fn add_constrained(&mut self, owner: OwnerId, pod_id: PodId) {
        if self.in_queue.insert(pod_id) {
            self.constrained.entry(owner).or_default().push_back(pod_id);
        }
    }

    fn add_unconstrained(&mut self, pod_id: PodId) {
        if self.in_queue.insert(pod_id) {
            self.unconstrained.push_back(pod_id);
        }
    }

    fn remove(&mut self, pod_id: PodId) {
        self.in_queue.remove(&pod_id);
        // Lazy removal: stale entries are skipped when popped.
    }

    fn contains(&self, pod_id: PodId) -> bool {
        self.in_queue.contains(&pod_id)
    }
}

// ── Combined event handler ──────────────────────────────────────

struct SimHandler {
    scheduler: Scheduler,
    metrics: MetricsCollector,
    catalog: Catalog,
    overhead: Resources,
    daemonset_pct: u32,
    node_startup_ns: u64,
    node_startup_jitter_ns: u64,
    pod_startup_ns: u64,
    pod_startup_jitter_ns: u64,
    /// Seeded RNG for delay jitter. None = no jitter.
    rng: Option<rand::rngs::StdRng>,
    /// Counter for generating unique node hostnames.
    node_counter: u32,
    /// Scheduling strategy (FullScan, HintBased, Partitioned).
    strategy: SchedulingStrategy,
    /// HintBased: maps pod to its expected node launch index.
    pod_node_hints: std::collections::HashMap<PodId, usize>,
    /// HintBased: NodeIds in launch order.
    launched_nodes: Vec<NodeId>,
    /// Budget-based domain tracking for ReverseSchedule constraint enforcement.
    domain_budget: DomainBudget,
    /// Capacity-waiting queue for O(1) NodeReady in ReverseSchedule.
    wait_queue: CapacityWaitQueue,
}

impl SimHandler {
    /// Compute a delay with optional uniform jitter.
    fn jittered_delay(&mut self, base_ns: u64, jitter_ns: u64) -> u64 {
        if jitter_ns == 0 {
            return base_ns;
        }
        if let Some(ref mut rng) = self.rng {
            use rand::Rng;
            let j = rng.gen_range(0..=jitter_ns * 2) as i64 - jitter_ns as i64;
            (base_ns as i64 + j).max(1) as u64
        } else {
            base_ns
        }
    }
}

impl kubesim_engine::EventHandler for SimHandler {
    fn handle(
        &mut self,
        event: &EngineEvent,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<kubesim_engine::ScheduledEvent> {
        // Forward to metrics collector
        let mut follow_ups = self.metrics.handle(event, time, state);

        let mut result = match event {
            EngineEvent::PodSubmitted(spec) => {
                let duration_ns = spec.duration_ns;
                let pod = Pod {
                    requests: spec.requests,
                    limits: spec.limits,
                    phase: PodPhase::Pending,
                    node: None,
                    scheduling_constraints: spec.scheduling_constraints.clone(),
                    deletion_cost: None,
                    owner: spec.owner,
                    qos_class: QoSClass::Burstable,
                    priority: spec.priority,
                    labels: spec.labels.clone(),
                    do_not_disrupt: spec.do_not_disrupt,
                    duration_ns,
                    is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                };
                let pod_id = state.submit_pod(pod);

                // Register spread/anti-affinity constraints in budget for ReverseSchedule
                if self.strategy == SchedulingStrategy::ReverseSchedule {
                    for constraint in &spec.scheduling_constraints.topology_spread {
                        if constraint.when_unsatisfiable == WhenUnsatisfiable::DoNotSchedule {
                            self.domain_budget.register(
                                spec.owner,
                                constraint.topology_key.clone(),
                                constraint.max_skew,
                            );
                        }
                    }
                    for term in &spec.scheduling_constraints.pod_affinity {
                        if term.anti && matches!(term.affinity_type, AffinityType::Required)
                            && term.topology_key == "kubernetes.io/hostname"
                        {
                            if spec.labels.matches(&term.label_selector) {
                                self.domain_budget.register(
                                    spec.owner,
                                    term.topology_key.clone(),
                                    1,
                                );
                            }
                        }
                    }
                }

                // Short-circuit: skip scheduling if no ready nodes exist
                let has_ready_nodes = state.nodes.iter().any(|(_, n)| n.conditions.ready && !n.cordoned);
                if has_ready_nodes {
                    if let kubesim_scheduler::ScheduleResult::Bound(node_id) =
                        self.scheduler.schedule_one(state, pod_id)
                    {
                        state.bind_pod(pod_id, node_id);
                        // Schedule PodCompleted if duration is set
                        if let Some(dur) = duration_ns {
                            return vec![kubesim_engine::ScheduledEvent {
                                time: SimTime(time.0 + dur),
                                event: EngineEvent::PodCompleted(pod_id),
                            }];
                        }
                        return follow_ups;
                    }
                }
                // Pod unschedulable — handle wait queue and trigger provisioning
                {
                    // Pod couldn't be scheduled — add to wait queue for ReverseSchedule
                    if self.strategy == SchedulingStrategy::ReverseSchedule {
                        let has_constraints = !spec.scheduling_constraints.topology_spread.is_empty()
                            || spec.scheduling_constraints.pod_affinity.iter().any(|t| {
                                t.anti && matches!(t.affinity_type, AffinityType::Required)
                                    && t.topology_key == "kubernetes.io/hostname"
                            });
                        if has_constraints {
                            self.wait_queue.add_constrained(spec.owner, pod_id);
                        } else {
                            self.wait_queue.add_unconstrained(pod_id);
                        }
                    }
                    // Trigger provisioning
                    return vec![kubesim_engine::ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: EngineEvent::KarpenterProvisioningLoop,
                    }];
                }
                Vec::new()
            }
            EngineEvent::NodeLaunching(spec) => {
                let mut node = instance_to_node_with_pct(&self.catalog, &spec.instance_type, &self.overhead, self.daemonset_pct);
                node.labels = spec.labels.clone();
                node.taints = spec.taints.iter().cloned().collect();
                node.pool_name = spec.pool_name.clone();
                node.do_not_disrupt = spec.do_not_disrupt;
                node.created_at = time;
                // Auto-assign kubernetes.io/hostname (unique per node) and zone labels
                let node_num = self.node_counter;
                self.node_counter += 1;
                node.labels.insert("kubernetes.io/hostname".into(), format!("node-{}", node_num));
                let zones = ["us-east-1a", "us-east-1b", "us-east-1c"];
                node.labels.insert("topology.kubernetes.io/zone".into(), zones[(node_num as usize) % zones.len()].into());
                if self.node_startup_ns > 0 {
                    node.conditions.ready = false; // not ready until NodeReady fires
                }
                let node_id = state.add_node(node);
                // HintBased: record launch order and snapshot pending pods as hints
                if self.strategy == SchedulingStrategy::HintBased {
                    let launch_idx = self.launched_nodes.len();
                    self.launched_nodes.push(node_id);
                    for &pid in &state.pending_queue {
                        self.pod_node_hints.entry(pid).or_insert(launch_idx);
                    }
                }
                // Incrementally update scheduler caches — new topology domain
                if let Some(n) = state.nodes.get(node_id) {
                    self.scheduler.on_node_added(n);
                }
                // Try to schedule pending pods onto the new node (only if instant startup)
                // Skip for ReverseSchedule — scheduling happens in NodeReady via budget.
                if self.node_startup_ns == 0 && self.strategy != SchedulingStrategy::ReverseSchedule {
                    let pending: Vec<_> = state.pending_queue.clone();
                    self.scheduler.schedule_pending_from(state, &pending);
                }
                let startup_delay = self.jittered_delay(self.node_startup_ns, self.node_startup_jitter_ns);
                vec![kubesim_engine::ScheduledEvent {
                    time: SimTime(time.0 + startup_delay.max(1)),
                    event: EngineEvent::NodeReady(node_id),
                }]
            }
            EngineEvent::NodeReady(node_id) => {
                // Mark node ready and schedule pending pods onto it
                if let Some(n) = state.nodes.get_mut(*node_id) {
                    n.conditions.ready = true;
                }
                match self.strategy {
                    SchedulingStrategy::FullScan => {
                        let pending: Vec<_> = state.pending_queue.clone();
                        self.scheduler.schedule_pending_from(state, &pending);
                    }
                    SchedulingStrategy::HintBased => {
                        // Bind hinted pods for this node first
                        if let Some(launch_idx) = self.launched_nodes.iter().position(|&nid| nid == *node_id) {
                            let hinted: Vec<PodId> = self.pod_node_hints.iter()
                                .filter(|(_, &idx)| idx == launch_idx)
                                .map(|(&pid, _)| pid)
                                .collect();
                            if !hinted.is_empty() {
                                self.scheduler.schedule_pending_from(state, &hinted);
                            }
                            for pid in &hinted {
                                self.pod_node_hints.remove(pid);
                            }
                        }
                        // Fall back to normal scheduling for remaining pending pods
                        if !state.pending_queue.is_empty() {
                            let remaining: Vec<_> = state.pending_queue.clone();
                            self.scheduler.schedule_pending_from(state, &remaining);
                        }
                    }
                    SchedulingStrategy::Partitioned => {
                        let pending: Vec<_> = state.pending_queue.clone();
                        let (unconstrained, constrained): (Vec<_>, Vec<_>) = pending.iter().partition(|&&pid| {
                            state.pods.get(pid).map_or(true, |p| {
                                p.scheduling_constraints.topology_spread.is_empty()
                                && !p.scheduling_constraints.pod_affinity.iter().any(|t| t.anti && matches!(t.affinity_type, AffinityType::Required))
                            })
                        });
                        if !unconstrained.is_empty() {
                            let ids: Vec<PodId> = unconstrained.into_iter().copied().collect();
                            self.scheduler.schedule_pending_from(state, &ids);
                        }
                        let mut seen_owners = std::collections::HashSet::new();
                        let deduped: Vec<PodId> = constrained.into_iter()
                            .filter(|&&pid| state.pods.get(pid).map_or(false, |p| seen_owners.insert(p.owner)))
                            .copied()
                            .collect();
                        if !deduped.is_empty() {
                            self.scheduler.schedule_pending_from(state, &deduped);
                        }
                    }
                    SchedulingStrategy::NodePruning => {
                        let pending: Vec<_> = state.pending_queue.clone();
                        let before = state.pending_queue.len();
                        self.scheduler.schedule_pending_from(state, &pending);
                        // After scheduling, mark nodes that are full
                        let bound_count = before - state.pending_queue.len();
                        if bound_count > 0 {
                            let min_cpu = state.pending_queue.iter()
                                .filter_map(|&pid| state.pods.get(pid))
                                .map(|p| p.requests.cpu_millis)
                                .min()
                                .unwrap_or(0);
                            for (nid, node) in state.nodes.iter() {
                                if !node.conditions.ready || node.cordoned { continue; }
                                let avail = node.allocatable.saturating_sub(&node.allocated);
                                if avail.cpu_millis < min_cpu {
                                    self.scheduler.mark_saturated(nid);
                                }
                            }
                        }
                    }
                    SchedulingStrategy::ReverseSchedule => {
                        let hostname = state.nodes.get(*node_id)
                            .and_then(|n| n.labels.get("kubernetes.io/hostname").map(|s| s.to_string()))
                            .unwrap_or_default();

                        // Register new node's domain in all budgets
                        if !hostname.is_empty() {
                            self.domain_budget.register_domain("kubernetes.io/hostname", &hostname);
                        }

                        let mut to_bind: Vec<PodId> = Vec::new();
                        if let Some(node) = state.nodes.get(*node_id) {
                            let mut remaining = node.allocatable.saturating_sub(&node.allocated);

                            // Phase 1: Place constrained pods using budget
                            let owners: Vec<OwnerId> = self.wait_queue.constrained.keys().copied().collect();
                            for owner in owners {
                                if remaining.cpu_millis == 0 { break; }
                                let queue = match self.wait_queue.constrained.get_mut(&owner) {
                                    Some(q) if !q.is_empty() => q,
                                    _ => continue,
                                };
                                // Pop stale entries from front
                                while let Some(&pod_id) = queue.front() {
                                    if !self.wait_queue.in_queue.contains(&pod_id) {
                                        queue.pop_front();
                                        continue;
                                    }
                                    match state.pods.get(pod_id) {
                                        Some(p) if p.phase == PodPhase::Pending => break,
                                        _ => { queue.pop_front(); self.wait_queue.in_queue.remove(&pod_id); continue; }
                                    }
                                }
                                let pod_id = match queue.front().copied() {
                                    Some(id) => id,
                                    None => continue,
                                };
                                let pod = match state.pods.get(pod_id) {
                                    Some(p) => p,
                                    None => { queue.pop_front(); self.wait_queue.in_queue.remove(&pod_id); continue; }
                                };
                                if !pod.requests.fits_in(&remaining) { continue; }

                                // Check budget for all constraint keys
                                let mut budget_ok = true;
                                for c in &pod.scheduling_constraints.topology_spread {
                                    if c.when_unsatisfiable == WhenUnsatisfiable::DoNotSchedule
                                        && !self.domain_budget.has_room(owner, &c.topology_key, &hostname)
                                    {
                                        budget_ok = false;
                                        break;
                                    }
                                }
                                if budget_ok {
                                    for t in &pod.scheduling_constraints.pod_affinity {
                                        if t.anti && matches!(t.affinity_type, AffinityType::Required)
                                            && t.topology_key == "kubernetes.io/hostname"
                                            && pod.labels.matches(&t.label_selector)
                                            && !self.domain_budget.has_room(owner, "kubernetes.io/hostname", &hostname)
                                        {
                                            budget_ok = false;
                                            break;
                                        }
                                    }
                                }
                                if !budget_ok { continue; }

                                // Record placement
                                for c in &pod.scheduling_constraints.topology_spread {
                                    if c.when_unsatisfiable == WhenUnsatisfiable::DoNotSchedule {
                                        self.domain_budget.record_placement(owner, &c.topology_key, &hostname);
                                    }
                                }
                                for t in &pod.scheduling_constraints.pod_affinity {
                                    if t.anti && matches!(t.affinity_type, AffinityType::Required)
                                        && t.topology_key == "kubernetes.io/hostname"
                                    {
                                        self.domain_budget.record_placement(owner, "kubernetes.io/hostname", &hostname);
                                    }
                                }

                                queue.pop_front();
                                self.wait_queue.in_queue.remove(&pod_id);
                                remaining = remaining.saturating_sub(&pod.requests);
                                to_bind.push(pod_id);
                            }

                            // Phase 2: Fill remaining capacity with unconstrained pods
                            while remaining.cpu_millis > 0 {
                                let pod_id = match self.wait_queue.unconstrained.pop_front() {
                                    Some(id) => id,
                                    None => break,
                                };
                                if !self.wait_queue.in_queue.remove(&pod_id) { continue; }
                                let pod = match state.pods.get(pod_id) {
                                    Some(p) if p.phase == PodPhase::Pending => p,
                                    _ => continue,
                                };
                                if pod.requests.fits_in(&remaining) {
                                    remaining = remaining.saturating_sub(&pod.requests);
                                    to_bind.push(pod_id);
                                } else {
                                    // Put it back — node is full
                                    self.wait_queue.unconstrained.push_front(pod_id);
                                    self.wait_queue.in_queue.insert(pod_id);
                                    break;
                                }
                            }
                        }

                        for pid in &to_bind {
                            state.bind_pod(*pid, *node_id);
                        }
                    }
                }
                // Trigger provisioning if pods are still pending
                if !state.pending_queue.is_empty() {
                    return vec![kubesim_engine::ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: EngineEvent::KarpenterProvisioningLoop,
                    }];
                }
                Vec::new()
            }
            EngineEvent::KarpenterProvisioningLoop => {
                // Try to schedule pending pods onto existing nodes before the
                // provisioner launches new ones. This catches evicted pods that
                // were returned to pending by DrainHandler.
                // Skip for ReverseSchedule — scheduling happens in NodeReady via budget.
                if self.strategy == SchedulingStrategy::ReverseSchedule {
                    // Re-enqueue evicted pods that landed back in pending_queue
                    for &pod_id in &state.pending_queue {
                        if self.wait_queue.contains(pod_id) { continue; }
                        let pod = match state.pods.get(pod_id) {
                            Some(p) if p.phase == PodPhase::Pending => p,
                            _ => continue,
                        };
                        let has_constraints = !pod.scheduling_constraints.topology_spread.is_empty()
                            || pod.scheduling_constraints.pod_affinity.iter().any(|t| {
                                t.anti && matches!(t.affinity_type, AffinityType::Required)
                                    && t.topology_key == "kubernetes.io/hostname"
                            });
                        if has_constraints {
                            self.wait_queue.add_constrained(pod.owner, pod_id);
                        } else {
                            self.wait_queue.add_unconstrained(pod_id);
                        }
                    }
                } else {
                    let pending: Vec<_> = state.pending_queue.clone();
                    self.scheduler.schedule_pending_from(state, &pending);
                }
                Vec::new()
            }
            EngineEvent::NodeCordoned(node_id) => {
                if let Some(node) = state.nodes.get_mut(*node_id) {
                    node.cordoned = true;
                }
                Vec::new()
            }
            EngineEvent::NodeDrained(_node_id) => {
                // Eviction is handled by DrainHandler (registered as engine handler).
                Vec::new()
            }
            EngineEvent::NodeTerminated(node_id) => {
                state.remove_node(*node_id);
                // Invalidate scheduler caches — topology domains changed
                self.scheduler.invalidate_caches();
                self.scheduler.clear_saturated();
                Vec::new()
            }
            EngineEvent::PodCompleted(pod_id) => {
                // Remove from wait queue if present (stale entry)
                self.wait_queue.remove(*pod_id);
                if let Some(pod) = state.pods.get_mut(*pod_id) {
                    pod.phase = PodPhase::Succeeded;
                    if let Some(node_id) = pod.node.take() {
                        if let Some(node) = state.nodes.get_mut(node_id) {
                            node.allocated = node.allocated.saturating_sub(&pod.requests);
                            node.pods.retain(|p| *p != *pod_id);
                        }
                    }
                }
                Vec::new()
            }
            EngineEvent::PodResize(pod_id, new_requests) => {
                state.resize_pod(*pod_id, *new_requests);
                Vec::new()
            }
            EngineEvent::PodResizeByOwner(owner_id, new_requests) => {
                let pod_ids = state.running_pods_for_owner(*owner_id);
                for pid in pod_ids {
                    state.resize_pod(pid, *new_requests);
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
        result.append(&mut follow_ups);
        result
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

// ── Single simulation run ───────────────────────────────────────

/// Compute Shannon entropy and normalized form from a slice of counts.
fn shannon_entropy(counts: &[f64]) -> (f64, f64) {
    let total: f64 = counts.iter().sum();
    if total == 0.0 || counts.len() < 2 {
        return (0.0, 0.0);
    }
    let h: f64 = counts.iter()
        .filter(|&&c| c > 0.0)
        .map(|&c| { let p = c / total; -p * p.ln() })
        .sum();
    let max_h = (counts.len() as f64).ln();
    let normalized = if max_h > 0.0 { h / max_h } else { 0.0 };
    (h, normalized)
}

struct SimRunResult {
    events_processed: u64,
    total_cost_per_hour: f64,
    node_count: u32,
    pod_count: u32,
    running_pods: u32,
    pending_pods: u32,
    final_time: u64,
    pod_placement_entropy: f64,
    pod_placement_entropy_normalized: f64,
    cpu_weighted_entropy: f64,
    cpu_weighted_entropy_normalized: f64,
    // Cumulative / time-integrated metrics
    cumulative_cost: f64,
    time_weighted_node_count: f64,
    time_to_stable: f64,
    cumulative_pending_pod_seconds: f64,
    disruption_count: u64,
    disruption_seconds: f64,
    peak_node_count: u32,
    peak_cost_rate: f64,
    cumulative_vcpu_hours: f64,
    cumulative_memory_gib_hours: f64,
    // Raw timeseries snapshots
    timeseries: Vec<kubesim_metrics::MetricsSnapshot>,
}

/// Compute cumulative/time-integrated metrics from a series of snapshots.
///
/// In `WallClock` mode, `SimTime` values are nanoseconds.
/// In `Logical` mode, each tick is treated as 1 second.
fn compute_cumulative(snapshots: &[kubesim_metrics::MetricsSnapshot], time_mode: TimeMode) -> (f64, f64, f64, f64, u64, f64, u32, f64, f64, f64) {
    if snapshots.is_empty() {
        return (0.0, 0.0, 0.0, 0.0, 0, 0.0, 0, 0.0, 0.0, 0.0);
    }

    // Convert raw SimTime delta to seconds based on time mode
    let to_secs = |raw: f64| -> f64 {
        match time_mode {
            TimeMode::WallClock => raw / 1e9,
            TimeMode::Logical => raw, // each tick = 1 second
        }
    };

    let mut cumulative_cost = 0.0f64;
    let mut time_weighted_nodes = 0.0f64;
    let mut cumulative_pending_seconds = 0.0f64;
    let mut disruption_seconds = 0.0f64;
    let mut peak_node_count = 0u32;
    let mut peak_cost_rate = 0.0f64;
    let mut cumulative_vcpu_hours = 0.0f64;
    let mut cumulative_memory_gib_hours = 0.0f64;

    // Time-to-stable: last time node_count changed
    let mut time_to_stable = 0.0f64;
    let mut prev_node_count: Option<u32> = None;

    for i in 0..snapshots.len() {
        let s = &snapshots[i];
        let t_secs = to_secs(s.time.0 as f64);

        if s.node_count > peak_node_count { peak_node_count = s.node_count; }
        if s.total_cost_per_hour > peak_cost_rate { peak_cost_rate = s.total_cost_per_hour; }

        if let Some(prev_nc) = prev_node_count {
            if s.node_count != prev_nc {
                time_to_stable = t_secs;
            }
        }
        prev_node_count = Some(s.node_count);

        // Trapezoidal integration between consecutive snapshots
        if i > 0 {
            let prev = &snapshots[i - 1];
            let dt_raw = s.time.0 as f64 - prev.time.0 as f64;
            let dt_secs = to_secs(dt_raw);
            let dt_hours = dt_secs / 3600.0;

            // Cost integral: cost_rate ($/hr) * dt (hr) = $
            cumulative_cost += (prev.total_cost_per_hour + s.total_cost_per_hour) / 2.0 * dt_hours;
            // vCPU-hours: vCPU * hours
            cumulative_vcpu_hours += (prev.total_vcpu_allocated + s.total_vcpu_allocated) / 2.0 * dt_hours;
            // GiB-hours: GiB * hours
            cumulative_memory_gib_hours += (prev.total_memory_allocated_gib + s.total_memory_allocated_gib) / 2.0 * dt_hours;
            // Node-count integral: nodes * seconds
            time_weighted_nodes += (prev.node_count as f64 + s.node_count as f64) / 2.0 * dt_secs;
            // Pending-pod-seconds
            cumulative_pending_seconds += (prev.pending_count as f64 + s.pending_count as f64) / 2.0 * dt_secs;
            // Disruption-seconds (disruption_count is cumulative, so use delta)
            let d_prev = prev.disruption_count as f64;
            let d_curr = s.disruption_count as f64;
            if d_curr > d_prev {
                disruption_seconds += (d_curr - d_prev) * dt_secs;
            }
        }
    }

    let final_disruptions = snapshots.last().map_or(0, |s| s.disruption_count);

    (cumulative_cost, time_weighted_nodes, time_to_stable, cumulative_pending_seconds,
     final_disruptions, disruption_seconds, peak_node_count, peak_cost_rate,
     cumulative_vcpu_hours, cumulative_memory_gib_hours)
}

fn run_single(
    workload_events: &[WorkloadEvent],
    variant: Option<&Variant>,
    scenario: &ScenarioFile,
    time_mode: TimeMode,
    seed: u64,
    event_budget: u64,
) -> SimRunResult {
    let provider = scenario.study.catalog_provider;
    let catalog = Catalog::for_provider(provider).expect("embedded catalog");
    let overhead = compute_overhead(&scenario.study.cluster);
    let daemonset_pct = scenario.study.cluster.daemonset_overhead_percent.unwrap_or(0);

    let mut state = ClusterState::new();
    let mut engine = Engine::new(time_mode);

    let scoring = variant
        .and_then(|v| v.scheduler.as_ref())
        .map(|s| scoring_from_workload(s.scoring))
        .unwrap_or(ScoringStrategy::LeastAllocated);

    // Seed engine from workload events
    for we in workload_events {
        match we {
            WorkloadEvent::NodeLaunching { instance_type, pool_index, .. } => {
                let mut node = instance_to_node_with_pct(&catalog, instance_type, &overhead, daemonset_pct);
                node.pool_name = resolve_pool_name(&scenario.study.cluster.node_pools, *pool_index);
                let node_id = state.add_node(node);
                engine.schedule(SimTime(0), EngineEvent::NodeReady(node_id));
            }
            WorkloadEvent::PodSubmitted { time, requests, limits, priority, owner_id, workload_name, duration_ns, scheduling_constraints, labels, .. } => {
                engine.schedule(*time, EngineEvent::PodSubmitted(PodSpec {
                    requests: *requests,
                    limits: *limits,
                    owner: OwnerId(*owner_id),
                    priority: *priority,
                    labels: labels.clone(),
                    scheduling_constraints: scheduling_constraints.clone(),
                    do_not_disrupt: workload_name == "batch_job",
                    duration_ns: *duration_ns,
                }));
            }
            WorkloadEvent::MetricsSnapshot { time } => {
                engine.schedule(*time, EngineEvent::MetricsSnapshot);
            }
            WorkloadEvent::HpaEvaluation { time, owner_id } => {
                engine.schedule(
                    *time,
                    EngineEvent::HpaEvaluation(kubesim_engine::DeploymentId(*owner_id)),
                );
            }
            WorkloadEvent::KarpenterProvisioningLoop { time } => {
                engine.schedule(*time, EngineEvent::KarpenterProvisioningLoop);
            }
            WorkloadEvent::KarpenterConsolidationLoop { time } => {
                engine.schedule(*time, EngineEvent::KarpenterConsolidationLoop);
            }
            WorkloadEvent::SpotInterruptionCheck { time } => {
                engine.schedule(*time, EngineEvent::SpotInterruptionCheck);
            }
            WorkloadEvent::ReplicaSetSubmitted {
                time, owner_id, desired_replicas, requests, limits, priority, deletion_cost_strategy, scheduling_constraints, labels,
            } => {
                let owner = OwnerId(*owner_id);
                state.add_replica_set(ReplicaSet {
                    owner_id: owner,
                    desired_replicas: *desired_replicas,
                    pod_template: PodTemplate {
                        requests: *requests,
                        limits: *limits,
                        priority: *priority,
                        labels: labels.clone(),
                        scheduling_constraints: scheduling_constraints.clone(),
                    },
                    deletion_cost_strategy: *deletion_cost_strategy,
                });
                engine.schedule(*time, EngineEvent::ReplicaSetReconcile(owner));
            }
            WorkloadEvent::ReplicaSetScaleDown { time, owner_id, reduce_by } => {
                engine.schedule(
                    *time,
                    EngineEvent::ScaleDown(kubesim_engine::DeploymentId(*owner_id), *reduce_by),
                );
            }
            WorkloadEvent::ReplicaSetScaleUp { time, owner_id, increase_to } => {
                // increase_to is absolute target; ScaleUp engine event adds to current.
                // We pass increase_to and let the RS handler set desired_replicas directly.
                engine.schedule(
                    *time,
                    EngineEvent::ScaleUp(kubesim_engine::DeploymentId(*owner_id), *increase_to),
                );
            }
            WorkloadEvent::PodResize { time, owner_id, new_requests } => {
                engine.schedule(
                    *time,
                    EngineEvent::PodResizeByOwner(OwnerId(*owner_id), *new_requests),
                );
            }
            _ => {}
        }
    }

    let delays = &scenario.study.cluster.delays;
    let has_jitter = delays.node_startup_jitter_ns() > 0
        || delays.pod_startup_jitter_ns() > 0
        || delays.provisioner_batch_jitter_ns() > 0;
    let handler = SimHandler {
        scheduler: if has_jitter || true {
            // Always use seeded scheduler for reproducible tie-breaking when seed varies
            Scheduler::with_seed(SchedulerProfile::with_scoring("default", scoring), seed)
        } else {
            Scheduler::new(SchedulerProfile::with_scoring("default", scoring))
        },
        metrics: MetricsCollector::new(RustMetricsConfig::default()),
        catalog: Catalog::for_provider(provider).expect("embedded catalog"),
        overhead,
        daemonset_pct,
        node_startup_ns: delays.node_startup_ns(),
        node_startup_jitter_ns: delays.node_startup_jitter_ns(),
        pod_startup_ns: delays.pod_startup_ns(),
        pod_startup_jitter_ns: delays.pod_startup_jitter_ns(),
        rng: if has_jitter {
            Some(rand::rngs::StdRng::seed_from_u64(seed.wrapping_add(0xDE1A0)))
        } else {
            None
        },
        node_counter: 0,
        strategy: scenario.study.scheduling_strategy,
        pod_node_hints: std::collections::HashMap::new(),
        launched_nodes: Vec::new(),
        domain_budget: DomainBudget::new(),
        wait_queue: CapacityWaitQueue::new(),
    };
    engine.add_handler(Box::new(handler));
    engine.add_handler(Box::new(ReplicaSetController));
    engine.add_handler(Box::new(daemonset_handler_from_scenario(&scenario.study.cluster.daemonsets)));

    // Register karpenter handlers for pools that have karpenter config
    let version_profile = variant
        .and_then(|v| v.karpenter_version.as_deref())
        .and_then(parse_karpenter_version)
        .map(VersionProfile::new);

    // Build and sort pools by weight (higher weight = higher priority)
    let mut pool_defs: Vec<(usize, &kubesim_workload::NodePoolDef)> = scenario.study.cluster.node_pools
        .iter().enumerate().collect();
    pool_defs.sort_by(|a, b| b.1.weight.cmp(&a.1.weight));

    for (idx, pool_def) in pool_defs {
        if let Some(karpenter) = &pool_def.karpenter {
            let pool_name = pool_def.name.clone()
                .unwrap_or_else(|| if scenario.study.cluster.node_pools.len() == 1 {
                    "default".into()
                } else {
                    format!("pool-{}", idx)
                });
            let pool = nodepool_from_def(pool_def, pool_name, variant.and_then(|v| v.disruption_budget.as_ref()));

            let mut prov = ProvisioningHandler::new(
                Catalog::for_provider(provider).expect("embedded catalog"),
                pool.clone(),
            ).with_overhead(overhead).with_daemonset_pct(daemonset_pct);
            if time_mode == TimeMode::Logical {
                prov = prov.with_logical_mode();
            }
            let batch_ns = delays.provisioner_batch_ns();
            if batch_ns > 0 {
                prov.loop_interval_ns = batch_ns;
            }
            let batch_jitter_ns = delays.provisioner_batch_jitter_ns();
            if batch_jitter_ns > 0 {
                prov = prov.with_batch_jitter(batch_jitter_ns, seed);
            }
            if let Some(ref vp) = version_profile {
                prov = prov.with_version(vp.clone());
            }
            engine.add_handler(Box::new(prov));

            let consolidation_policy = karpenter
                .consolidation
                .as_ref()
                .map(|c| map_consolidation_policy(c.policy))
                .unwrap_or(ConsolidationPolicy::WhenUnderutilized);

            // Variant-level consolidate_when override
            let (effective_policy, effective_threshold) = match variant.and_then(|v| v.consolidate_when.as_ref()) {
                Some(cw) => (map_consolidation_policy(cw.policy), cw.decision_ratio_threshold.unwrap_or(1.0)),
                None => (consolidation_policy, karpenter.consolidation.as_ref().and_then(|c| c.decision_ratio_threshold).unwrap_or(1.0)),
            };

            let mut consol = ConsolidationHandler::new(pool, effective_policy)
                .with_catalog(Catalog::for_provider(provider).expect("embedded catalog"));
            consol.decision_ratio_threshold = effective_threshold;
            if time_mode == TimeMode::Logical {
                consol = consol.with_logical_mode();
            }
            consol.overhead = overhead;
            consol.daemonset_pct = daemonset_pct;
            if let Some(ref vp) = version_profile {
                let mut vp = vp.clone();
                let effective_db = variant.and_then(|v| v.disruption_budget.as_ref()).or(pool_def.disruption_budget.as_ref());
                if let Some(db) = effective_db {
                    vp.budgets = vec![kubesim_karpenter::version::DisruptionBudgetConfig {
                        max_percent: db.max_percent,
                        reasons: Vec::new(),
                        schedule: db.schedule.clone(),
                        active_budget: db.active_budget,
                        inactive_budget: db.inactive_budget,
                    }];
                }
                consol = consol.with_version(vp);
            }
            engine.add_handler(Box::new(consol));

            engine.add_handler(Box::new(DrainHandler));

            engine.add_handler(Box::new(SpotInterruptionHandler::new(seed)));
        }
    }

    // Wire DeletionCostController if variant specifies a strategy
    if let Some(strategy) = variant.and_then(|v| v.deletion_cost_strategy) {
        if strategy != DeletionCostStrategy::None {
            engine.add_handler(Box::new(DeletionCostController::new(strategy, time_mode)));
            // Schedule after initial pod creation (t=0 RS submit, t=1 reconcile)
            engine.schedule(SimTime(2), EngineEvent::DeletionCostReconcile);
        }
    }

    // Compute sim end time: last workload event + stabilization window.
    // In Logical mode, SimTime ticks represent ~1 second each, so use
    // 900 ticks (15 min). In WallClock mode, SimTime is nanoseconds.
    let max_event_time = workload_events.iter().map(|e| e.time().0).max().unwrap_or(0);
    let stabilization = match time_mode {
        TimeMode::Logical => 15 * 60,                    // 900 ticks ≈ 15 min
        TimeMode::WallClock => 15 * 60 * 1_000_000_000,  // 15 min in ns
    };
    let max_sim_time_ns = max_event_time + stabilization;

    let events_processed = engine.run_until_with_budget(&mut state, SimTime(max_sim_time_ns), event_budget);

    // Extract snapshots from SimHandler for cumulative metrics and timeseries
    let mut cumulative = (0.0, 0.0, 0.0, 0.0, 0u64, 0.0, 0u32, 0.0, 0.0, 0.0);
    let mut timeseries = Vec::new();
    for h in engine.handlers_mut() {
        if let Some(sh) = h.as_any_mut().downcast_mut::<SimHandler>() {
            cumulative = compute_cumulative(sh.metrics.snapshots(), time_mode);
            timeseries = sh.metrics.snapshots().to_vec();
            break;
        }
    }
    let (cumulative_cost, time_weighted_node_count, time_to_stable,
     cumulative_pending_pod_seconds, disruption_count, disruption_seconds,
     peak_node_count, peak_cost_rate, cumulative_vcpu_hours, cumulative_memory_gib_hours) = cumulative;

    // Collect final state summary
    let mut total_cost = 0.0f64;
    let mut node_count = 0u32;
    for (_id, node) in state.nodes.iter() {
        total_cost += node.cost_per_hour;
        node_count += 1;
    }

    let mut running = 0u32;
    let mut pending = 0u32;
    let mut pod_count = 0u32;
    for (_id, pod) in state.pods.iter() {
        pod_count += 1;
        match pod.phase {
            PodPhase::Running => running += 1,
            PodPhase::Pending => pending += 1,
            _ => {}
        }
    }

    // Entropy metrics
    let pod_counts: Vec<f64> = state.nodes.iter().map(|(_, n)| n.pods.len() as f64).collect();
    let (pod_placement_entropy, pod_placement_entropy_normalized) = shannon_entropy(&pod_counts);
    let cpu_allocs: Vec<f64> = state.nodes.iter().map(|(_, n)| n.allocated.cpu_millis as f64).collect();
    let (cpu_weighted_entropy, cpu_weighted_entropy_normalized) = shannon_entropy(&cpu_allocs);

    SimRunResult {
        events_processed,
        total_cost_per_hour: total_cost,
        node_count,
        pod_count,
        running_pods: running,
        pending_pods: pending,
        final_time: state.time.0,
        pod_placement_entropy,
        pod_placement_entropy_normalized,
        cpu_weighted_entropy,
        cpu_weighted_entropy_normalized,
        cumulative_cost,
        time_weighted_node_count,
        time_to_stable,
        cumulative_pending_pod_seconds,
        disruption_count,
        disruption_seconds,
        peak_node_count,
        peak_cost_rate,
        cumulative_vcpu_hours,
        cumulative_memory_gib_hours,
        timeseries,
    }
}

// ── Python types ────────────────────────────────────────────────

#[pyclass]
#[derive(Clone)]
struct SimResult {
    #[pyo3(get)]
    events_processed: u64,
    #[pyo3(get)]
    total_cost_per_hour: f64,
    #[pyo3(get)]
    node_count: u32,
    #[pyo3(get)]
    pod_count: u32,
    #[pyo3(get)]
    running_pods: u32,
    #[pyo3(get)]
    pending_pods: u32,
    #[pyo3(get)]
    final_time: u64,
    #[pyo3(get)]
    variant: String,
    #[pyo3(get)]
    seed: u64,
    #[pyo3(get)]
    pod_placement_entropy: f64,
    #[pyo3(get)]
    pod_placement_entropy_normalized: f64,
    #[pyo3(get)]
    cpu_weighted_entropy: f64,
    #[pyo3(get)]
    cpu_weighted_entropy_normalized: f64,
    #[pyo3(get)]
    cumulative_cost: f64,
    #[pyo3(get)]
    time_weighted_node_count: f64,
    #[pyo3(get)]
    time_to_stable: f64,
    #[pyo3(get)]
    cumulative_pending_pod_seconds: f64,
    #[pyo3(get)]
    disruption_count: u64,
    #[pyo3(get)]
    disruption_seconds: f64,
    #[pyo3(get)]
    peak_node_count: u32,
    #[pyo3(get)]
    peak_cost_rate: f64,
    #[pyo3(get)]
    cumulative_vcpu_hours: f64,
    #[pyo3(get)]
    cumulative_memory_gib_hours: f64,
    // Raw timeseries (not exposed via #[pyo3(get)] — use .timeseries property)
    timeseries_data: Vec<kubesim_metrics::MetricsSnapshot>,
}

/// Convert a MetricsSnapshot to a Python dict.
fn snapshot_to_dict<'py>(py: Python<'py>, s: &kubesim_metrics::MetricsSnapshot) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
    let d = pyo3::types::PyDict::new_bound(py);
    d.set_item("time", s.time.0)?;
    d.set_item("total_cost_per_hour", s.total_cost_per_hour)?;
    d.set_item("disruption_count", s.disruption_count)?;
    d.set_item("node_count", s.node_count)?;
    d.set_item("pod_count", s.pod_count)?;
    d.set_item("pending_count", s.pending_count)?;
    d.set_item("availability", s.availability)?;
    d.set_item("cpu_utilization_p50", s.cpu_utilization.p50)?;
    d.set_item("cpu_utilization_p90", s.cpu_utilization.p90)?;
    d.set_item("cpu_utilization_p99", s.cpu_utilization.p99)?;
    d.set_item("memory_utilization_p50", s.memory_utilization.p50)?;
    d.set_item("memory_utilization_p90", s.memory_utilization.p90)?;
    d.set_item("memory_utilization_p99", s.memory_utilization.p99)?;
    d.set_item("total_vcpu_allocated", s.total_vcpu_allocated)?;
    d.set_item("total_memory_allocated_gib", s.total_memory_allocated_gib)?;
    d.set_item("consolidation_decisions_total", s.consolidation_decisions_total)?;
    d.set_item("consolidation_decisions_accepted", s.consolidation_decisions_accepted)?;
    d.set_item("consolidation_decisions_rejected", s.consolidation_decisions_rejected)?;
    d.set_item("consolidation_decision_ratio_mean", s.consolidation_decision_ratio_mean)?;
    d.set_item("scale_down_terminations", s.scale_down_terminations)?;
    d.set_item("consolidation_evictions", s.consolidation_evictions)?;
    Ok(d)
}

#[pymethods]
impl SimResult {
    fn __repr__(&self) -> String {
        format!(
            "SimResult(variant={:?}, cost={:.4}, nodes={}, pods={}/{} running, events={})",
            self.variant, self.total_cost_per_hour, self.node_count,
            self.running_pods, self.pod_count, self.events_processed,
        )
    }

    /// Export as dict for polars/pandas DataFrame construction.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("seed", self.seed)?;
        dict.set_item("variant", &self.variant)?;
        dict.set_item("events_processed", self.events_processed)?;
        dict.set_item("total_cost_per_hour", self.total_cost_per_hour)?;
        dict.set_item("node_count", self.node_count)?;
        dict.set_item("pod_count", self.pod_count)?;
        dict.set_item("running_pods", self.running_pods)?;
        dict.set_item("pending_pods", self.pending_pods)?;
        dict.set_item("final_time", self.final_time)?;
        dict.set_item("pod_placement_entropy", self.pod_placement_entropy)?;
        dict.set_item("pod_placement_entropy_normalized", self.pod_placement_entropy_normalized)?;
        dict.set_item("cpu_weighted_entropy", self.cpu_weighted_entropy)?;
        dict.set_item("cpu_weighted_entropy_normalized", self.cpu_weighted_entropy_normalized)?;
        dict.set_item("cumulative_cost", self.cumulative_cost)?;
        dict.set_item("time_weighted_node_count", self.time_weighted_node_count)?;
        dict.set_item("time_to_stable", self.time_to_stable)?;
        dict.set_item("cumulative_pending_pod_seconds", self.cumulative_pending_pod_seconds)?;
        dict.set_item("disruption_count", self.disruption_count)?;
        dict.set_item("disruption_seconds", self.disruption_seconds)?;
        dict.set_item("peak_node_count", self.peak_node_count)?;
        dict.set_item("peak_cost_rate", self.peak_cost_rate)?;
        dict.set_item("cumulative_vcpu_hours", self.cumulative_vcpu_hours)?;
        dict.set_item("cumulative_memory_gib_hours", self.cumulative_memory_gib_hours)?;
        // Include timeseries as list of dicts
        let ts_list = pyo3::types::PyList::empty_bound(py);
        for s in &self.timeseries_data {
            ts_list.append(snapshot_to_dict(py, s)?)?;
        }
        dict.set_item("timeseries", ts_list)?;
        Ok(dict)
    }

    /// Return timeseries snapshots as a list of dicts.
    #[getter]
    fn timeseries<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyList>> {
        let list = pyo3::types::PyList::empty_bound(py);
        for s in &self.timeseries_data {
            list.append(snapshot_to_dict(py, s)?)?;
        }
        Ok(list)
    }
}

#[pyclass]
struct Simulation {
    scenario: ScenarioFile,
    workload_events: Vec<WorkloadEvent>,
    time_mode: TimeMode,
    seed: u64,
    event_budget: u64,
}

#[pymethods]
impl Simulation {
    #[new]
    #[pyo3(signature = (config, time_mode=None, seed=None, event_budget=None))]
    fn new(config: &str, time_mode: Option<&str>, seed: Option<u64>, event_budget: Option<u64>) -> PyResult<Self> {
        let yaml = if Path::new(config).exists() {
            std::fs::read_to_string(config)
                .map_err(|e| PyValueError::new_err(format!("failed to read config: {e}")))?
        } else {
            config.to_string()
        };

        let s = seed.unwrap_or(42);
        let (scenario, workload_events) = load_scenario_from_str_seeded(&yaml, s)
            .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;

        let tm = match time_mode {
            Some(s) => parse_time_mode(s)?,
            None => scenario_time_to_engine(scenario.study.time_mode),
        };

        Ok(Self {
            scenario,
            workload_events,
            time_mode: tm,
            seed: s,
            event_budget: event_budget.unwrap_or(10_000_000),
        })
    }

    /// Run simulation for a single variant.
    #[pyo3(signature = (variant=None))]
    fn run(&self, variant: Option<&str>) -> PyResult<SimResult> {
        let v = match variant {
            Some(name) => self.scenario.study.variants.iter()
                .find(|v| v.name == name)
                .ok_or_else(|| PyValueError::new_err(format!("variant {name:?} not found")))?,
            None => self.scenario.study.variants.first()
                .ok_or_else(|| PyValueError::new_err("no variants defined in scenario"))?,
        };

        let r = run_single(&self.workload_events, Some(v), &self.scenario, self.time_mode, self.seed, self.event_budget);

        Ok(SimResult {
            events_processed: r.events_processed,
            total_cost_per_hour: r.total_cost_per_hour,
            node_count: r.node_count,
            pod_count: r.pod_count,
            running_pods: r.running_pods,
            pending_pods: r.pending_pods,
            final_time: r.final_time,
            variant: v.name.clone(),
            seed: self.seed,
            pod_placement_entropy: r.pod_placement_entropy,
            pod_placement_entropy_normalized: r.pod_placement_entropy_normalized,
            cpu_weighted_entropy: r.cpu_weighted_entropy,
            cpu_weighted_entropy_normalized: r.cpu_weighted_entropy_normalized,
            cumulative_cost: r.cumulative_cost,
            time_weighted_node_count: r.time_weighted_node_count,
            time_to_stable: r.time_to_stable,
            cumulative_pending_pod_seconds: r.cumulative_pending_pod_seconds,
            disruption_count: r.disruption_count,
            disruption_seconds: r.disruption_seconds,
            peak_node_count: r.peak_node_count,
            peak_cost_rate: r.peak_cost_rate,
            cumulative_vcpu_hours: r.cumulative_vcpu_hours,
            cumulative_memory_gib_hours: r.cumulative_memory_gib_hours,
            timeseries_data: r.timeseries,
        })
    }

    /// Run all variants, returning a list of SimResult.
    fn run_all(&self) -> PyResult<Vec<SimResult>> {
        if self.scenario.study.variants.is_empty() {
            return Err(PyValueError::new_err("no variants defined in scenario"));
        }

        Ok(self.scenario.study.variants.iter().map(|v| {
            let r = run_single(&self.workload_events, Some(v), &self.scenario, self.time_mode, self.seed, self.event_budget);
            SimResult {
                events_processed: r.events_processed,
                total_cost_per_hour: r.total_cost_per_hour,
                node_count: r.node_count,
                pod_count: r.pod_count,
                running_pods: r.running_pods,
                pending_pods: r.pending_pods,
                final_time: r.final_time,
                variant: v.name.clone(),
                seed: self.seed,
                pod_placement_entropy: r.pod_placement_entropy,
                pod_placement_entropy_normalized: r.pod_placement_entropy_normalized,
                cpu_weighted_entropy: r.cpu_weighted_entropy,
                cpu_weighted_entropy_normalized: r.cpu_weighted_entropy_normalized,
                cumulative_cost: r.cumulative_cost,
                time_weighted_node_count: r.time_weighted_node_count,
                time_to_stable: r.time_to_stable,
                cumulative_pending_pod_seconds: r.cumulative_pending_pod_seconds,
                disruption_count: r.disruption_count,
                disruption_seconds: r.disruption_seconds,
                peak_node_count: r.peak_node_count,
                peak_cost_rate: r.peak_cost_rate,
                cumulative_vcpu_hours: r.cumulative_vcpu_hours,
                cumulative_memory_gib_hours: r.cumulative_memory_gib_hours,
                timeseries_data: r.timeseries,
            }
        }).collect())
    }
}

/// Batch run a scenario across multiple seeds with rayon parallelism.
///
/// Returns a list of dicts suitable for `polars.DataFrame(batch_run(...))`.
#[pyfunction]
#[pyo3(signature = (config, seeds, parallelism=None, event_budget=None))]
fn batch_run<'py>(
    py: Python<'py>,
    config: &str,
    seeds: Vec<u64>,
    parallelism: Option<usize>,
    event_budget: Option<u64>,
) -> PyResult<Bound<'py, pyo3::types::PyList>> {
    let yaml = if Path::new(config).exists() {
        std::fs::read_to_string(config)
            .map_err(|e| PyValueError::new_err(format!("failed to read config: {e}")))?
    } else {
        config.to_string()
    };

    // Parse scenario once for validation and variant info; events regenerated per-seed
    let (scenario, _) = load_scenario_from_str(&yaml)
        .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;

    let time_mode = scenario_time_to_engine(scenario.study.time_mode);

    if scenario.study.variants.is_empty() {
        return Err(PyValueError::new_err("no variants defined in scenario"));
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallelism.unwrap_or(0))
        .build()
        .map_err(|e| PyValueError::new_err(format!("failed to create thread pool: {e}")))?;

    // Build work items: (seed, variant_index)
    let work: Vec<(u64, usize)> = seeds.iter()
        .flat_map(|&s| (0..scenario.study.variants.len()).map(move |vi| (s, vi)))
        .collect();

    let budget = event_budget.unwrap_or(10_000_000);

    // Run in parallel, releasing the GIL
    // Each seed generates its own workload events from the distributions
    let results: Vec<(u64, String, SimRunResult)> = py.allow_threads(|| {
        pool.install(|| {
            work.par_iter().map(|&(seed, vi)| {
                let v = &scenario.study.variants[vi];
                let events = load_scenario_from_str_seeded(&yaml, seed)
                    .expect("scenario already validated")
                    .1;
                let r = run_single(&events, Some(v), &scenario, time_mode, seed, budget);
                (seed, v.name.clone(), r)
            }).collect()
        })
    });

    // Convert to list of dicts
    let list = pyo3::types::PyList::empty_bound(py);
    for (seed, variant_name, r) in &results {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("seed", seed)?;
        dict.set_item("variant", variant_name)?;
        dict.set_item("events_processed", r.events_processed)?;
        dict.set_item("total_cost_per_hour", r.total_cost_per_hour)?;
        dict.set_item("node_count", r.node_count)?;
        dict.set_item("pod_count", r.pod_count)?;
        dict.set_item("running_pods", r.running_pods)?;
        dict.set_item("pending_pods", r.pending_pods)?;
        dict.set_item("final_time", r.final_time)?;
        dict.set_item("pod_placement_entropy", r.pod_placement_entropy)?;
        dict.set_item("pod_placement_entropy_normalized", r.pod_placement_entropy_normalized)?;
        dict.set_item("cpu_weighted_entropy", r.cpu_weighted_entropy)?;
        dict.set_item("cpu_weighted_entropy_normalized", r.cpu_weighted_entropy_normalized)?;
        dict.set_item("cumulative_cost", r.cumulative_cost)?;
        dict.set_item("time_weighted_node_count", r.time_weighted_node_count)?;
        dict.set_item("time_to_stable", r.time_to_stable)?;
        dict.set_item("cumulative_pending_pod_seconds", r.cumulative_pending_pod_seconds)?;
        dict.set_item("disruption_count", r.disruption_count)?;
        dict.set_item("disruption_seconds", r.disruption_seconds)?;
        dict.set_item("peak_node_count", r.peak_node_count)?;
        dict.set_item("peak_cost_rate", r.peak_cost_rate)?;
        dict.set_item("cumulative_vcpu_hours", r.cumulative_vcpu_hours)?;
        dict.set_item("cumulative_memory_gib_hours", r.cumulative_memory_gib_hours)?;
        // Include timeseries as list of dicts
        let ts_list = pyo3::types::PyList::empty_bound(py);
        for s in &r.timeseries {
            ts_list.append(snapshot_to_dict(py, s)?)?;
        }
        dict.set_item("timeseries", ts_list)?;
        list.append(dict)?;
    }

    Ok(list)
}

// ── Step-based simulation for Gymnasium ─────────────────────────

/// Observation returned after each step of the step-based simulation.
#[pyclass]
#[derive(Clone)]
struct StepObs {
    /// Per-node CPU utilization (allocated / allocatable), length = node_count.
    #[pyo3(get)]
    cpu_utils: Vec<f64>,
    /// Per-node memory utilization, length = node_count.
    #[pyo3(get)]
    mem_utils: Vec<f64>,
    /// Number of pending pods.
    #[pyo3(get)]
    pending_pods: u32,
    /// Total hourly cost of all nodes.
    #[pyo3(get)]
    cost_rate: f64,
    /// Fraction of non-terminal pods that are Running.
    #[pyo3(get)]
    availability: f64,
    /// Number of active nodes.
    #[pyo3(get)]
    node_count: u32,
    /// Number of active pods.
    #[pyo3(get)]
    pod_count: u32,
    /// Disruptions this step.
    #[pyo3(get)]
    disruptions: u64,
}

#[pymethods]
impl StepObs {
    fn __repr__(&self) -> String {
        format!(
            "StepObs(nodes={}, pods={}, pending={}, cost={:.3}, avail={:.3})",
            self.node_count, self.pod_count, self.pending_pods, self.cost_rate, self.availability,
        )
    }
}

/// Step-based simulation for Gymnasium integration.
///
/// Holds mutable engine + state. Each `step()` advances simulation by a time
/// window, applies the agent's action (scheduling weights), and returns an
/// observation.
#[pyclass(unsendable)]
struct StepSimulation {
    // Stored scenario for reset
    scenario: ScenarioFile,
    workload_events: Vec<WorkloadEvent>,
    time_mode: TimeMode,
    step_duration: u64,
    max_steps: u64,
    // Mutable sim state (None before first reset)
    engine: Option<Engine>,
    state: Option<ClusterState>,
    current_step: u64,
}

#[pymethods]
impl StepSimulation {
    #[new]
    #[pyo3(signature = (config, step_duration=60_000_000_000u64, max_steps=100, time_mode=None, _seed=None))]
    fn new(
        config: &str,
        step_duration: u64,
        max_steps: u64,
        time_mode: Option<&str>,
        _seed: Option<u64>,
    ) -> PyResult<Self> {
        let yaml = if Path::new(config).exists() {
            std::fs::read_to_string(config)
                .map_err(|e| PyValueError::new_err(format!("failed to read config: {e}")))?
        } else {
            config.to_string()
        };
        let s = _seed.unwrap_or(42);
        let (scenario, workload_events) = load_scenario_from_str_seeded(&yaml, s)
            .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;
        let tm = match time_mode {
            Some(s) => parse_time_mode(s)?,
            None => scenario_time_to_engine(scenario.study.time_mode),
        };
        Ok(Self {
            scenario,
            workload_events,
            time_mode: tm,
            step_duration,
            max_steps,
            engine: None,
            state: None,
            current_step: 0,
        })
    }

    /// Reset the simulation. Returns initial observation.
    #[pyo3(signature = (variant=None))]
    fn reset(&mut self, variant: Option<&str>) -> PyResult<StepObs> {
        let provider = self.scenario.study.catalog_provider;
        let catalog = Catalog::for_provider(provider)
            .map_err(|e| PyValueError::new_err(format!("catalog: {e}")))?;

        let v = match variant {
            Some(name) => self.scenario.study.variants.iter()
                .find(|v| v.name == name),
            None => self.scenario.study.variants.first(),
        };

        let scoring = v
            .and_then(|v| v.scheduler.as_ref())
            .map(|s| scoring_from_workload(s.scoring))
            .unwrap_or(ScoringStrategy::LeastAllocated);

        let mut state = ClusterState::new();
        let mut engine = Engine::new(self.time_mode);
        let overhead = compute_overhead(&self.scenario.study.cluster);
        let daemonset_pct = self.scenario.study.cluster.daemonset_overhead_percent.unwrap_or(0);

        for we in &self.workload_events {
            match we {
                WorkloadEvent::NodeLaunching { instance_type, pool_index, .. } => {
                    let mut node = instance_to_node_with_pct(&catalog, instance_type, &overhead, daemonset_pct);
                    node.pool_name = resolve_pool_name(&self.scenario.study.cluster.node_pools, *pool_index);
                    let node_id = state.add_node(node);
                    engine.schedule(SimTime(0), EngineEvent::NodeReady(node_id));
                }
                WorkloadEvent::PodSubmitted { time, requests, limits, priority, owner_id, workload_name, duration_ns, .. } => {
                    engine.schedule(*time, EngineEvent::PodSubmitted(PodSpec {
                        requests: *requests,
                        limits: *limits,
                        owner: OwnerId(*owner_id),
                        priority: *priority,
                        labels: LabelSet::default(),
                        scheduling_constraints: SchedulingConstraints::default(),
                        do_not_disrupt: workload_name == "batch_job",
                        duration_ns: *duration_ns,
                    }));
                }
                WorkloadEvent::MetricsSnapshot { time } => {
                    engine.schedule(*time, EngineEvent::MetricsSnapshot);
                }
                WorkloadEvent::HpaEvaluation { time, owner_id } => {
                    engine.schedule(
                        *time,
                        EngineEvent::HpaEvaluation(kubesim_engine::DeploymentId(*owner_id)),
                    );
                }
                WorkloadEvent::KarpenterProvisioningLoop { time } => {
                    engine.schedule(*time, EngineEvent::KarpenterProvisioningLoop);
                }
                WorkloadEvent::KarpenterConsolidationLoop { time } => {
                    engine.schedule(*time, EngineEvent::KarpenterConsolidationLoop);
                }
                WorkloadEvent::SpotInterruptionCheck { time } => {
                    engine.schedule(*time, EngineEvent::SpotInterruptionCheck);
                }
                WorkloadEvent::ReplicaSetSubmitted {
                    time, owner_id, desired_replicas, requests, limits, priority, deletion_cost_strategy, scheduling_constraints, labels,
                } => {
                    let owner = OwnerId(*owner_id);
                    state.add_replica_set(ReplicaSet {
                        owner_id: owner,
                        desired_replicas: *desired_replicas,
                        pod_template: PodTemplate {
                            requests: *requests,
                            limits: *limits,
                            priority: *priority,
                            labels: labels.clone(),
                            scheduling_constraints: scheduling_constraints.clone(),
                        },
                        deletion_cost_strategy: *deletion_cost_strategy,
                    });
                    engine.schedule(*time, EngineEvent::ReplicaSetReconcile(owner));
                }
                WorkloadEvent::ReplicaSetScaleDown { time, owner_id, reduce_by } => {
                    engine.schedule(
                        *time,
                        EngineEvent::ScaleDown(kubesim_engine::DeploymentId(*owner_id), *reduce_by),
                    );
                }
                WorkloadEvent::ReplicaSetScaleUp { time, owner_id, increase_to } => {
                    engine.schedule(
                        *time,
                        EngineEvent::ScaleUp(kubesim_engine::DeploymentId(*owner_id), *increase_to),
                    );
                }
                WorkloadEvent::PodResize { time, owner_id, new_requests } => {
                    engine.schedule(
                        *time,
                        EngineEvent::PodResizeByOwner(OwnerId(*owner_id), *new_requests),
                    );
                }
                _ => {}
            }
        }

        // Add the combined handler (scheduler + metrics) to the engine
        let delays = &self.scenario.study.cluster.delays;
        let handler = SimHandler {
            scheduler: Scheduler::with_seed(
                SchedulerProfile::with_scoring("default", scoring),
                42, // StepSimulation uses fixed seed
            ),
            metrics: MetricsCollector::new(RustMetricsConfig::default()),
            catalog: Catalog::for_provider(provider)
                .map_err(|e| PyValueError::new_err(format!("catalog: {e}")))?,
            overhead,
            daemonset_pct,
            node_startup_ns: delays.node_startup_ns(),
            node_startup_jitter_ns: delays.node_startup_jitter_ns(),
            pod_startup_ns: delays.pod_startup_ns(),
            pod_startup_jitter_ns: delays.pod_startup_jitter_ns(),
            rng: if delays.node_startup_jitter_ns() > 0 || delays.pod_startup_jitter_ns() > 0 {
                Some(rand::rngs::StdRng::seed_from_u64(42u64.wrapping_add(0xDE1A0)))
            } else {
                None
            },
            node_counter: 0,
            strategy: self.scenario.study.scheduling_strategy,
            pod_node_hints: std::collections::HashMap::new(),
            launched_nodes: Vec::new(),
            domain_budget: DomainBudget::new(),
            wait_queue: CapacityWaitQueue::new(),
        };
        engine.add_handler(Box::new(handler));
        engine.add_handler(Box::new(ReplicaSetController));
        engine.add_handler(Box::new(daemonset_handler_from_scenario(&self.scenario.study.cluster.daemonsets)));

        // Register karpenter handlers for pools that have karpenter config
        // Sort pools by weight (higher weight = higher priority)
        let mut pool_defs: Vec<(usize, &kubesim_workload::NodePoolDef)> = self.scenario.study.cluster.node_pools
            .iter().enumerate().collect();
        pool_defs.sort_by(|a, b| b.1.weight.cmp(&a.1.weight));

        for (idx, pool_def) in pool_defs {
            if let Some(karpenter) = &pool_def.karpenter {
                let pool_name = pool_def.name.clone()
                    .unwrap_or_else(|| if self.scenario.study.cluster.node_pools.len() == 1 {
                        "default".into()
                    } else {
                        format!("pool-{}", idx)
                    });
                let pool = nodepool_from_def(pool_def, pool_name, None);

                engine.add_handler(Box::new(
                    ProvisioningHandler::new(
                        Catalog::for_provider(provider).map_err(|e| PyValueError::new_err(format!("catalog: {e}")))?,
                        pool.clone(),
                    ).with_overhead(overhead).with_daemonset_pct(daemonset_pct),
                ));

                let consolidation_policy = karpenter
                    .consolidation
                    .as_ref()
                    .map(|c| map_consolidation_policy(c.policy))
                    .unwrap_or(ConsolidationPolicy::WhenUnderutilized);

                engine.add_handler(Box::new(
                    ConsolidationHandler::new(pool, consolidation_policy)
                        .with_catalog(Catalog::for_provider(provider).map_err(|e| PyValueError::new_err(format!("catalog: {e}")))?),
                ));

                engine.add_handler(Box::new(DrainHandler));

                engine.add_handler(Box::new(SpotInterruptionHandler::new(42)));
            }
        }

        self.engine = Some(engine);
        self.state = Some(state);
        self.current_step = 0;

        Ok(self.observe())
    }

    /// Advance simulation by one time window.
    ///
    /// `action` is `[scoring_weight, consolidation_threshold, scale_target]`:
    /// - scoring_weight: 0.0 = LeastAllocated, 1.0 = MostAllocated (blended)
    /// - consolidation_threshold: 0.0–1.0 utilization below which to consolidate
    /// - scale_target: 0.0–1.0 target utilization for scaling decisions
    ///
    /// Returns `(obs, reward, terminated, truncated, info_dict)`.
    fn step<'py>(
        &mut self,
        py: Python<'py>,
        action: Vec<f64>,
    ) -> PyResult<(StepObs, f64, bool, bool, Bound<'py, pyo3::types::PyDict>)> {
        if self.engine.is_none() {
            return Err(PyValueError::new_err("call reset() first"));
        }

        // Action values (reserved for future use with dynamic scheduler switching)
        let _scoring_weight = action.first().copied().unwrap_or(0.5);
        let _consolidation_thresh = action.get(1).copied().unwrap_or(0.5);
        let _scale_target = action.get(2).copied().unwrap_or(0.7);

        // Advance simulation by step_duration
        let step_dur = self.step_duration;
        {
            let engine = self.engine.as_mut().unwrap();
            let state = self.state.as_mut().unwrap();
            let until = SimTime(state.time.0 + step_dur);
            engine.run_until(state, until);
            state.time = until;
        }

        self.current_step += 1;
        let obs = self.observe();

        let reward = -obs.cost_rate * 0.01
            - obs.disruptions as f64 * 10.0
            + obs.availability * 5.0;

        let engine = self.engine.as_ref().unwrap();
        let state = self.state.as_ref().unwrap();
        let terminated = engine.pending() == 0 && state.pending_queue.is_empty();
        let truncated = self.current_step >= self.max_steps;

        let info = pyo3::types::PyDict::new_bound(py);
        info.set_item("step", self.current_step)?;
        info.set_item("sim_time", state.time.0)?;
        info.set_item("events_remaining", engine.pending())?;

        Ok((obs, reward, terminated, truncated, info))
    }

    /// Number of nodes in current state.
    fn node_count(&self) -> u32 {
        self.state.as_ref().map_or(0, |s| s.nodes.len())
    }

    /// Number of pods in current state.
    fn pod_count(&self) -> u32 {
        self.state.as_ref().map_or(0, |s| s.pods.len())
    }
}

impl StepSimulation {
    fn observe(&mut self) -> StepObs {
        let state = self.state.as_ref().unwrap();

        let mut cpu_utils = Vec::new();
        let mut mem_utils = Vec::new();
        let mut cost_rate = 0.0f64;
        let mut node_count = 0u32;

        for (_id, node) in state.nodes.iter() {
            node_count += 1;
            cost_rate += node.cost_per_hour;
            let cpu = if node.allocatable.cpu_millis > 0 {
                node.allocated.cpu_millis as f64 / node.allocatable.cpu_millis as f64
            } else { 0.0 };
            let mem = if node.allocatable.memory_bytes > 0 {
                node.allocated.memory_bytes as f64 / node.allocatable.memory_bytes as f64
            } else { 0.0 };
            cpu_utils.push(cpu);
            mem_utils.push(mem);
        }

        let mut running = 0u32;
        let mut active = 0u32;
        let mut pending = 0u32;
        let mut pod_count = 0u32;

        for (_id, pod) in state.pods.iter() {
            pod_count += 1;
            match pod.phase {
                PodPhase::Running => { running += 1; active += 1; }
                PodPhase::Pending => { pending += 1; active += 1; }
                PodPhase::Terminating => { active += 1; }
                _ => {}
            }
        }

        let availability = if active > 0 { running as f64 / active as f64 } else { 1.0 };

        // Disruptions are not directly accessible from the handler inside the
        // engine, so we approximate from pending count changes.
        StepObs {
            cpu_utils,
            mem_utils,
            pending_pods: pending,
            cost_rate,
            availability,
            node_count,
            pod_count,
            disruptions: 0,
        }
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Simulation>()?;
    m.add_class::<SimResult>()?;
    m.add_class::<StepSimulation>()?;
    m.add_class::<StepObs>()?;
    m.add_function(wrap_pyfunction!(batch_run, m)?)?;
    Ok(())
}
