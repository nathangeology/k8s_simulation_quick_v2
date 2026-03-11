//! Karpenter Consolidation — WhenEmpty and WhenUnderutilized policies.
//!
//! Periodically evaluates nodes for consolidation opportunities:
//! - **WhenEmpty**: nodes with zero running pods are terminated immediately.
//! - **WhenUnderutilized**: nodes whose pods can all fit on other existing nodes
//!   are cordoned, drained, and terminated.
//! - **Replace** (v1.x): nodes whose pods don't fit elsewhere but could run on a
//!   cheaper instance type are replaced.

use kubesim_core::*;
use kubesim_ec2::Catalog;
use kubesim_engine::{Event, EventHandler, NodeSpec, ScheduledEvent};

use crate::nodepool::NodePool;
use crate::version::{ConsolidationStrategy, VersionProfile};

/// Consolidation policy selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConsolidationPolicy {
    WhenEmpty,
    WhenUnderutilized,
}

/// Result of evaluating a single node for consolidation.
#[derive(Debug)]
pub enum ConsolidationAction {
    /// Node is empty — terminate directly.
    TerminateEmpty(NodeId),
    /// Node is underutilized — cordon, drain pods, then terminate.
    DrainAndTerminate {
        node_id: NodeId,
        pod_ids: Vec<PodId>,
    },
    /// v1.x: Replace a node with a cheaper instance type.
    Replace {
        node_id: NodeId,
        pod_ids: Vec<PodId>,
        replacement_instance_type: String,
    },
}

/// Identify empty nodes (ready, not cordoned, zero pods).
fn find_empty_nodes(state: &ClusterState) -> Vec<NodeId> {
    state
        .nodes
        .iter()
        .filter(|(_, n)| n.conditions.ready && !n.cordoned && n.pods.is_empty())
        .map(|(id, _)| id)
        .collect()
}

/// Check whether all pods on `candidate` can fit on other ready, non-cordoned nodes.
///
/// Uses a greedy first-fit approach: for each pod, find any other node with
/// enough available resources AND passing scheduling constraints (affinity,
/// taints, topology spread). Real Karpenter uses full scheduling simulation;
/// this approximates it by running the scheduler's filter plugins.
fn pods_can_reschedule(
    state: &ClusterState,
    candidate: NodeId,
) -> Option<Vec<PodId>> {
    use kubesim_scheduler::{
        FilterResult, TaintToleration, NodeAffinity,
        InterPodAffinityFilter, PodTopologySpreadFilter, FilterPlugin,
    };

    let node = state.nodes.get(candidate)?;
    if node.pods.is_empty() {
        return Some(Vec::new());
    }

    let filters: Vec<Box<dyn FilterPlugin>> = vec![
        Box::new(TaintToleration),
        Box::new(NodeAffinity),
        Box::new(InterPodAffinityFilter),
        Box::new(PodTopologySpreadFilter),
    ];

    // Collect pods to move
    let pod_ids: Vec<PodId> = node.pods.iter().copied().collect();
    let pods: Vec<(PodId, &Pod)> = pod_ids
        .iter()
        .filter_map(|&pid| state.pods.get(pid).map(|p| (pid, p)))
        .collect();

    // Build available capacity on other nodes (mutable copy for greedy allocation)
    let mut other_avail: Vec<(NodeId, Resources)> = state
        .nodes
        .iter()
        .filter(|(nid, n)| *nid != candidate && n.conditions.ready && !n.cordoned)
        .map(|(nid, n)| (nid, n.allocatable.saturating_sub(&n.allocated)))
        .collect();

    for &(_, pod) in &pods {
        let slot = other_avail.iter_mut().find(|(nid, avail)| {
            // Check resource fit
            if !pod.requests.fits_in(avail) {
                return false;
            }
            // Check scheduling constraints via filter plugins
            let target_node = match state.nodes.get(*nid) {
                Some(n) => n,
                None => return false,
            };
            filters.iter().all(|f| matches!(f.filter(state, pod, target_node), FilterResult::Pass))
        });
        match slot {
            Some((_, avail)) => *avail = avail.saturating_sub(&pod.requests),
            None => return None,
        }
    }

    Some(pod_ids)
}

/// Find underutilized nodes whose pods can all be rescheduled elsewhere.
fn find_underutilized_nodes(state: &ClusterState) -> Vec<ConsolidationAction> {
    let mut actions = Vec::new();
    // Sort candidates by cost ascending — consolidate cheapest nodes first
    let mut candidates: Vec<(NodeId, f64)> = state
        .nodes
        .iter()
        .filter(|(_, n)| n.conditions.ready && !n.cordoned && !n.pods.is_empty())
        .map(|(id, n)| (id, n.cost_per_hour))
        .collect();
    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    for (nid, _) in candidates {
        if let Some(pod_ids) = pods_can_reschedule(state, nid) {
            actions.push(ConsolidationAction::DrainAndTerminate {
                node_id: nid,
                pod_ids,
            });
        }
    }
    actions
}

/// Find nodes that can't be deleted (pods don't fit elsewhere) but could be
/// replaced with a cheaper instance type from the EC2 catalog.
///
/// For each candidate node (ready, not cordoned, has pods), if `pods_can_reschedule`
/// fails, check whether a cheaper instance type exists that fits all the node's pods.
fn find_replace_candidates(
    state: &ClusterState,
    catalog: &Catalog,
    pool: &NodePool,
) -> Vec<ConsolidationAction> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter()
            .filter_map(|name| catalog.get(name))
            .collect()
    };

    let mut actions = Vec::new();

    let mut candidates: Vec<(NodeId, &Node)> = state
        .nodes
        .iter()
        .filter(|(_, n)| n.conditions.ready && !n.cordoned && !n.pods.is_empty())
        .collect();
    candidates.sort_by(|a, b| b.1.cost_per_hour.partial_cmp(&a.1.cost_per_hour).unwrap_or(std::cmp::Ordering::Equal));

    for (nid, node) in candidates {
        // Skip if pods can already be rescheduled (delete path handles these)
        if pods_can_reschedule(state, nid).is_some() {
            continue;
        }

        // Compute total resource demand of pods on this node
        let mut total_cpu: u64 = 0;
        let mut total_mem: u64 = 0;
        let mut total_gpu: u32 = 0;
        let pod_ids: Vec<PodId> = node.pods.iter().copied().collect();
        for &pid in &pod_ids {
            if let Some(p) = state.pods.get(pid) {
                total_cpu += p.requests.cpu_millis;
                total_mem += p.requests.memory_bytes;
                total_gpu = total_gpu.max(p.requests.gpu);
            }
        }

        // Find cheapest instance type that fits all pods and is cheaper than current node
        let current_cost = node.cost_per_hour;
        let mut best: Option<(&kubesim_ec2::InstanceType, f64)> = None;

        for it in &allowed {
            let it_cpu = (it.vcpu as u64) * 1000;
            let it_mem = (it.memory_gib as u64) * 1024 * 1024 * 1024;
            if it_cpu < total_cpu || it_mem < total_mem || it.gpu_count < total_gpu {
                continue;
            }
            let price = it.on_demand_price_per_hour;
            if price >= current_cost {
                continue;
            }
            if best.as_ref().map_or(true, |(_, bp)| price < *bp) {
                best = Some((it, price));
            }
        }

        if let Some((it, _)) = best {
            actions.push(ConsolidationAction::Replace {
                node_id: nid,
                pod_ids,
                replacement_instance_type: it.instance_type.clone(),
            });
        }
    }

    actions
}

/// Run one consolidation evaluation, returning actions to take.
/// Respects the disruption budget: at most `max_disrupted` nodes may be
/// disrupted in a single pass.
pub fn evaluate(
    state: &ClusterState,
    policy: ConsolidationPolicy,
    max_disrupted: u32,
) -> Vec<ConsolidationAction> {
    evaluate_versioned(state, policy, max_disrupted, None, None)
}

/// Version-aware consolidation evaluation.
/// When `profile` is `None`, uses v1.x default behavior.
/// When `catalog` is provided and replace_consolidation is enabled, evaluates
/// the replace path for nodes that can't be consolidated by deletion.
pub fn evaluate_versioned(
    state: &ClusterState,
    policy: ConsolidationPolicy,
    max_disrupted: u32,
    profile: Option<&VersionProfile>,
    catalog: Option<(&Catalog, &NodePool)>,
) -> Vec<ConsolidationAction> {
    let strategy = profile
        .map(|p| p.consolidation_strategy)
        .unwrap_or(ConsolidationStrategy::MultiNode);
    let replace_enabled = profile
        .map(|p| p.replace_consolidation)
        .unwrap_or(true);

    let mut actions: Vec<ConsolidationAction> = Vec::new();
    let mut budget = max_disrupted;

    // WhenEmpty always runs (both policies include it)
    for nid in find_empty_nodes(state) {
        if budget == 0 {
            break;
        }
        actions.push(ConsolidationAction::TerminateEmpty(nid));
        budget -= 1;
    }

    if policy == ConsolidationPolicy::WhenUnderutilized && budget > 0 {
        // Delete path: nodes whose pods fit on existing nodes
        for action in find_underutilized_nodes(state) {
            if budget == 0 {
                break;
            }
            actions.push(action);
            budget -= 1;
        }

        // Replace path (v1.x only): nodes that can't be deleted but can be
        // swapped for a cheaper instance type
        if budget > 0 && replace_enabled && strategy == ConsolidationStrategy::MultiNode {
            if let Some((cat, pool)) = catalog {
                for action in find_replace_candidates(state, cat, pool) {
                    if budget == 0 {
                        break;
                    }
                    actions.push(action);
                    budget -= 1;
                }
            }
        }
    }

    actions
}

/// Compute the max number of nodes that may be disrupted given the pool config
/// and current node count.
pub fn disruption_budget(pool: &NodePool, total_nodes: u32) -> u32 {
    ((total_nodes as u64 * pool.max_disrupted_pct as u64) / 100).max(1) as u32
}

// ── EventHandler integration ────────────────────────────────────

/// Karpenter consolidation handler for the simulation engine.
pub struct ConsolidationHandler {
    pub pool: NodePool,
    pub policy: ConsolidationPolicy,
    /// Interval (ns) between consolidation loops in WallClock mode.
    pub loop_interval_ns: u64,
    /// Version profile controlling consolidation strategy.
    pub version_profile: Option<VersionProfile>,
    /// EC2 catalog for replace-path instance selection.
    pub catalog: Option<Catalog>,
}

impl ConsolidationHandler {
    pub fn new(pool: NodePool, policy: ConsolidationPolicy) -> Self {
        Self {
            pool,
            policy,
            loop_interval_ns: 30_000_000_000, // 30s default
            version_profile: None,
            catalog: None,
        }
    }

    /// Create a handler with a specific Karpenter version profile.
    pub fn with_version(mut self, profile: VersionProfile) -> Self {
        self.version_profile = Some(profile);
        self
    }

    /// Attach an EC2 catalog for consolidation replace-path evaluation.
    pub fn with_catalog(mut self, catalog: Catalog) -> Self {
        self.catalog = Some(catalog);
        self
    }
}

impl EventHandler for ConsolidationHandler {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        let Event::KarpenterConsolidationLoop = event else {
            return Vec::new();
        };

        let total_nodes = state.nodes.len();
        let max_d = disruption_budget(&self.pool, total_nodes);
        let catalog_ref = self.catalog.as_ref().map(|c| (c, &self.pool));
        let actions = evaluate_versioned(state, self.policy, max_d, self.version_profile.as_ref(), catalog_ref);
        let mut follow_ups = Vec::new();

        for action in actions {
            match action {
                ConsolidationAction::TerminateEmpty(nid) => {
                    // Cordon then terminate (no drain needed — node is empty)
                    if let Some(n) = state.nodes.get_mut(nid) {
                        n.cordoned = true;
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: Event::NodeCordoned(nid),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 2),
                        event: Event::NodeTerminated(nid),
                    });
                }
                ConsolidationAction::DrainAndTerminate { node_id, pod_ids } => {
                    // Cordon
                    if let Some(n) = state.nodes.get_mut(node_id) {
                        n.cordoned = true;
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: Event::NodeCordoned(node_id),
                    });
                    // Evict pods (they return to pending queue for rescheduling)
                    for pid in pod_ids {
                        state.evict_pod(pid);
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 2),
                        event: Event::NodeDrained(node_id),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 3),
                        event: Event::NodeTerminated(node_id),
                    });
                }
                ConsolidationAction::Replace { node_id, pod_ids, replacement_instance_type } => {
                    // v1.x replacement: cordon old node, drain, terminate, launch replacement
                    if let Some(n) = state.nodes.get_mut(node_id) {
                        n.cordoned = true;
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: Event::NodeCordoned(node_id),
                    });
                    for pid in pod_ids {
                        state.evict_pod(pid);
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 2),
                        event: Event::NodeDrained(node_id),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 3),
                        event: Event::NodeTerminated(node_id),
                    });
                    // Launch cheaper replacement
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + 4),
                        event: Event::NodeLaunching(NodeSpec {
                            instance_type: replacement_instance_type,
                            labels: kubesim_core::LabelSet(self.pool.labels.clone()),
                            taints: self.pool.taints.clone(),
                        }),
                    });
                }
            }
        }

        // Re-schedule next consolidation loop
        follow_ups.push(ScheduledEvent {
            time: SimTime(time.0 + self.loop_interval_ns),
            event: Event::KarpenterConsolidationLoop,
        });

        follow_ups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodepool::NodePoolLimits;

    fn test_node(cpu: u64, mem: u64) -> Node {
        Node {
            instance_type: "m5.xlarge".into(),
            allocatable: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            allocated: Resources::default(),
            pods: smallvec::smallvec![],
            conditions: NodeConditions { ready: true, ..Default::default() },
            labels: LabelSet::default(),
            taints: smallvec::smallvec![],
            cost_per_hour: 0.192,
            lifecycle: NodeLifecycle::OnDemand,
            cordoned: false,
        }
    }

    fn test_pod(cpu: u64, mem: u64) -> Pod {
        Pod {
            requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            limits: Resources::default(),
            phase: PodPhase::Pending,
            node: None,
            scheduling_constraints: SchedulingConstraints::default(),
            deletion_cost: None,
            owner: OwnerId(0),
            qos_class: QoSClass::Burstable,
            priority: 0,
            labels: LabelSet::default(),
        }
    }

    fn test_pool() -> NodePool {
        NodePool {
            name: "default".into(),
            instance_types: vec![],
            limits: NodePoolLimits::default(),
            labels: vec![],
            taints: vec![],
            max_disrupted_pct: 10,
        }
    }

    #[test]
    fn when_empty_terminates_empty_nodes() {
        let mut state = ClusterState::new();
        state.add_node(test_node(4000, 8_000_000_000)); // empty node

        let actions = evaluate(&state, ConsolidationPolicy::WhenEmpty, 10);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], ConsolidationAction::TerminateEmpty(_)));
    }

    #[test]
    fn when_empty_skips_nodes_with_pods() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));
        state.bind_pod(pid, nid);

        let actions = evaluate(&state, ConsolidationPolicy::WhenEmpty, 10);
        assert!(actions.is_empty());
    }

    #[test]
    fn when_underutilized_drains_reschedulable_nodes() {
        let mut state = ClusterState::new();
        // Node A: small pod
        let na = state.add_node(Node { cost_per_hour: 0.1, ..test_node(4000, 8_000_000_000) });
        let pid = state.submit_pod(test_pod(500, 500_000_000));
        state.bind_pod(pid, na);

        // Node B: has capacity to absorb A's pod
        state.add_node(test_node(4000, 8_000_000_000));

        let actions = evaluate(&state, ConsolidationPolicy::WhenUnderutilized, 10);
        assert!(!actions.is_empty());
        // Should drain the cheaper node
        let has_drain = actions.iter().any(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. }));
        assert!(has_drain);
    }

    #[test]
    fn disruption_budget_limits_actions() {
        let mut state = ClusterState::new();
        // 3 empty nodes
        state.add_node(test_node(4000, 8_000_000_000));
        state.add_node(test_node(4000, 8_000_000_000));
        state.add_node(test_node(4000, 8_000_000_000));

        // Budget of 1
        let actions = evaluate(&state, ConsolidationPolicy::WhenEmpty, 1);
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn disruption_budget_calculation() {
        let pool = test_pool();
        assert_eq!(disruption_budget(&pool, 100), 10); // 10% of 100
        assert_eq!(disruption_budget(&pool, 1), 1);    // min 1
        assert_eq!(disruption_budget(&pool, 0), 1);    // min 1
    }

    #[test]
    fn consolidation_handler_schedules_follow_ups() {
        let mut state = ClusterState::new();
        state.add_node(test_node(4000, 8_000_000_000)); // empty node

        let mut handler = ConsolidationHandler::new(test_pool(), ConsolidationPolicy::WhenEmpty);
        let events = handler.handle(
            &kubesim_engine::Event::KarpenterConsolidationLoop,
            SimTime(1000),
            &mut state,
        );
        // Should have NodeCordoned, NodeTerminated, and re-schedule
        assert!(events.len() >= 2);
        let has_reschedule = events.iter().any(|e| matches!(e.event, kubesim_engine::Event::KarpenterConsolidationLoop));
        assert!(has_reschedule);
    }

    #[test]
    fn consolidation_handler_ignores_non_consolidation_events() {
        let mut state = ClusterState::new();
        let mut handler = ConsolidationHandler::new(test_pool(), ConsolidationPolicy::WhenEmpty);
        let events = handler.handle(
            &kubesim_engine::Event::MetricsSnapshot,
            SimTime(1000),
            &mut state,
        );
        assert!(events.is_empty());
    }
}
