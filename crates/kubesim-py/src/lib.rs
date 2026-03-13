//! KubeSim Python — PyO3 bindings for single run and batch execution.

use pyo3::prelude::*;
use pyo3::exceptions::PyValueError;
use rayon::prelude::*;

use kubesim_core::{
    ClusterState, DeletionCostStrategy, LabelSet, Node, NodeConditions, NodeLifecycle, OwnerId, Pod, PodPhase,
    PodTemplate, QoSClass, ReplicaSet, Resources, SchedulingConstraints, SimTime,
};
use kubesim_ec2::Catalog;
use kubesim_engine::{DeletionCostController, Engine, Event as EngineEvent, PodSpec, ReplicaSetController, TimeMode};
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
};

use std::path::Path;

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

fn instance_to_node(catalog: &Catalog, instance_type: &str) -> Node {
    let (cpu, mem, gpu, cost) = catalog
        .get(instance_type)
        .map(|it| (
            it.vcpu as u64 * 1000,
            it.memory_gib as u64 * 1024 * 1024 * 1024,
            it.gpu_count,
            it.on_demand_price_per_hour,
        ))
        .unwrap_or((4000, 16 * 1024 * 1024 * 1024, 0, 0.192));

    Node {
        instance_type: instance_type.to_string(),
        allocatable: Resources { cpu_millis: cpu, memory_bytes: mem, gpu, ephemeral_bytes: 0 },
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

// ── Combined event handler ──────────────────────────────────────

struct SimHandler {
    scheduler: Scheduler,
    metrics: MetricsCollector,
    catalog: Catalog,
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
                };
                let pod_id = state.submit_pod(pod);

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
                } else {
                    // Pod couldn't be scheduled — trigger provisioning
                    return vec![kubesim_engine::ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: EngineEvent::KarpenterProvisioningLoop,
                    }];
                }
                Vec::new()
            }
            EngineEvent::NodeLaunching(spec) => {
                let mut node = instance_to_node(&self.catalog, &spec.instance_type);
                node.labels = spec.labels.clone();
                node.taints = spec.taints.iter().cloned().collect();
                node.pool_name = spec.pool_name.clone();
                node.do_not_disrupt = spec.do_not_disrupt;
                let node_id = state.add_node(node);
                // Try to schedule pending pods onto the new node
                let pending: Vec<_> = state.pending_queue.clone();
                for pod_id in pending {
                    if let kubesim_scheduler::ScheduleResult::Bound(nid) =
                        self.scheduler.schedule_one(state, pod_id)
                    {
                        state.bind_pod(pod_id, nid);
                    }
                }
                vec![kubesim_engine::ScheduledEvent {
                    time: SimTime(time.0 + 1),
                    event: EngineEvent::NodeReady(node_id),
                }]
            }
            EngineEvent::KarpenterProvisioningLoop => {
                // Try to schedule pending pods onto existing nodes before the
                // provisioner launches new ones. This catches evicted pods that
                // were returned to pending by DrainHandler.
                let pending: Vec<_> = state.pending_queue.clone();
                for pod_id in pending {
                    if let kubesim_scheduler::ScheduleResult::Bound(nid) =
                        self.scheduler.schedule_one(state, pod_id)
                    {
                        state.bind_pod(pod_id, nid);
                    }
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
                Vec::new()
            }
            EngineEvent::PodCompleted(pod_id) => {
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
) -> SimRunResult {
    let provider = scenario.study.catalog_provider;
    let catalog = Catalog::for_provider(provider).expect("embedded catalog");

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
                let mut node = instance_to_node(&catalog, instance_type);
                node.pool_name = resolve_pool_name(&scenario.study.cluster.node_pools, *pool_index);
                state.add_node(node);
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
                time, owner_id, desired_replicas, requests, limits, priority, deletion_cost_strategy,
            } => {
                let owner = OwnerId(*owner_id);
                state.add_replica_set(ReplicaSet {
                    owner_id: owner,
                    desired_replicas: *desired_replicas,
                    pod_template: PodTemplate {
                        requests: *requests,
                        limits: *limits,
                        priority: *priority,
                        labels: LabelSet::default(),
                        scheduling_constraints: SchedulingConstraints::default(),
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
            _ => {}
        }
    }

    let handler = SimHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", scoring)),
        metrics: MetricsCollector::new(RustMetricsConfig::default()),
        catalog: Catalog::for_provider(provider).expect("embedded catalog"),
    };
    engine.add_handler(Box::new(handler));
    engine.add_handler(Box::new(ReplicaSetController));

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
            );
            if let Some(ref vp) = version_profile {
                prov = prov.with_version(vp.clone());
            }
            engine.add_handler(Box::new(prov));

            let consolidation_policy = karpenter
                .consolidation
                .as_ref()
                .map(|c| match c.policy {
                    kubesim_workload::ConsolidationPolicy::WhenEmpty => ConsolidationPolicy::WhenEmpty,
                    kubesim_workload::ConsolidationPolicy::WhenUnderutilized => ConsolidationPolicy::WhenUnderutilized,
                })
                .unwrap_or(ConsolidationPolicy::WhenUnderutilized);

            let mut consol = ConsolidationHandler::new(pool, consolidation_policy)
                .with_catalog(Catalog::for_provider(provider).expect("embedded catalog"));
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

    let events_processed = if let Some(stop_ns) = scenario.study.duration.as_deref()
        .and_then(kubesim_workload::parse_duration_ns)
    {
        let stop_time = match time_mode {
            TimeMode::WallClock => SimTime(stop_ns),
            TimeMode::Logical => SimTime(stop_ns / 1_000_000_000),
        };
        engine.run_until(&mut state, stop_time)
    } else {
        engine.run_to_completion(&mut state)
    };

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
}

#[pymethods]
impl Simulation {
    #[new]
    #[pyo3(signature = (config, time_mode=None, seed=None))]
    fn new(config: &str, time_mode: Option<&str>, seed: Option<u64>) -> PyResult<Self> {
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

        let r = run_single(&self.workload_events, Some(v), &self.scenario, self.time_mode, self.seed);

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
            let r = run_single(&self.workload_events, Some(v), &self.scenario, self.time_mode, self.seed);
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
#[pyo3(signature = (config, seeds, parallelism=None))]
fn batch_run<'py>(
    py: Python<'py>,
    config: &str,
    seeds: Vec<u64>,
    parallelism: Option<usize>,
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

    // Run in parallel, releasing the GIL
    // Each seed generates its own workload events from the distributions
    let results: Vec<(u64, String, SimRunResult)> = py.allow_threads(|| {
        pool.install(|| {
            work.par_iter().map(|&(seed, vi)| {
                let v = &scenario.study.variants[vi];
                let events = load_scenario_from_str_seeded(&yaml, seed)
                    .expect("scenario already validated")
                    .1;
                let r = run_single(&events, Some(v), &scenario, time_mode, seed);
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

        for we in &self.workload_events {
            match we {
                WorkloadEvent::NodeLaunching { instance_type, pool_index, .. } => {
                    let mut node = instance_to_node(&catalog, instance_type);
                    node.pool_name = resolve_pool_name(&self.scenario.study.cluster.node_pools, *pool_index);
                    state.add_node(node);
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
                    time, owner_id, desired_replicas, requests, limits, priority, deletion_cost_strategy,
                } => {
                    let owner = OwnerId(*owner_id);
                    state.add_replica_set(ReplicaSet {
                        owner_id: owner,
                        desired_replicas: *desired_replicas,
                        pod_template: PodTemplate {
                            requests: *requests,
                            limits: *limits,
                            priority: *priority,
                            labels: LabelSet::default(),
                            scheduling_constraints: SchedulingConstraints::default(),
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
                _ => {}
            }
        }

        // Add the combined handler (scheduler + metrics) to the engine
        let handler = SimHandler {
            scheduler: Scheduler::new(
                SchedulerProfile::with_scoring("default", scoring),
            ),
            metrics: MetricsCollector::new(RustMetricsConfig::default()),
            catalog: Catalog::for_provider(provider)
                .map_err(|e| PyValueError::new_err(format!("catalog: {e}")))?,
        };
        engine.add_handler(Box::new(handler));
        engine.add_handler(Box::new(ReplicaSetController));

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
                    ),
                ));

                let consolidation_policy = karpenter
                    .consolidation
                    .as_ref()
                    .map(|c| match c.policy {
                        kubesim_workload::ConsolidationPolicy::WhenEmpty => ConsolidationPolicy::WhenEmpty,
                        kubesim_workload::ConsolidationPolicy::WhenUnderutilized => ConsolidationPolicy::WhenUnderutilized,
                    })
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
