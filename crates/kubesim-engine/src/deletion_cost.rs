//! DeletionCostController — periodic reconciler that sets pod deletion-cost
//! annotations based on node-level ranking strategies (from Karpenter PR 2894).
//!
//! All pods on a node get the same rank. Base rank starts at -1000, increments
//! per node in ranked order. Nodes with do-not-disrupt pods get higher costs
//! (protected group).

use kubesim_core::{
    ClusterState, DeletionCostStrategy, NodeId, PodId, PodPhase,
};

use crate::{Event, EventHandler, ScheduledEvent, SimTime, TimeMode};

const DO_NOT_DISRUPT_KEY: &str = "karpenter.sh/do-not-disrupt";
const BASE_RANK: i32 = -1000;

/// Periodic controller that sets `pod.deletion_cost` based on a ranking strategy.
pub struct DeletionCostController {
    pub strategy: DeletionCostStrategy,
    /// Interval (ns) between reconcile loops.
    pub loop_interval_ns: u64,
}

impl DeletionCostController {
    pub fn new(strategy: DeletionCostStrategy, time_mode: TimeMode) -> Self {
        let loop_interval_ns = match time_mode {
            TimeMode::Logical => 10,
            TimeMode::WallClock => 10_000_000_000, // 10s
        };
        Self {
            strategy,
            loop_interval_ns,
        }
    }
}

impl EventHandler for DeletionCostController {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        let Event::DeletionCostReconcile = event else {
            return Vec::new();
        };

        if self.strategy == DeletionCostStrategy::None {
            return vec![reschedule(time, self.loop_interval_ns)];
        }

        reconcile(state, self.strategy);

        vec![reschedule(time, self.loop_interval_ns)]
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

fn reschedule(time: SimTime, interval: u64) -> ScheduledEvent {
    ScheduledEvent {
        time: SimTime(time.0 + interval),
        event: Event::DeletionCostReconcile,
    }
}

/// Collect ready, non-cordoned nodes that have running pods.
fn active_nodes(state: &ClusterState) -> Vec<(NodeId, Vec<PodId>)> {
    state
        .nodes
        .iter()
        .filter(|(_, n)| n.conditions.ready && !n.cordoned)
        .filter_map(|(nid, n)| {
            let running: Vec<PodId> = n
                .pods
                .iter()
                .copied()
                .filter(|&pid| {
                    state
                        .pods
                        .get(pid)
                        .map_or(false, |p| p.phase == PodPhase::Running)
                })
                .collect();
            if running.is_empty() {
                None
            } else {
                Some((nid, running))
            }
        })
        .collect()
}

/// True if any pod on this node has the do-not-disrupt annotation.
fn has_do_not_disrupt(state: &ClusterState, pod_ids: &[PodId]) -> bool {
    pod_ids.iter().any(|&pid| {
        state
            .pods
            .get(pid)
            .map_or(false, |p| p.labels.get(DO_NOT_DISRUPT_KEY).is_some())
    })
}

fn reconcile(state: &mut ClusterState, strategy: DeletionCostStrategy) {
    if strategy == DeletionCostStrategy::PodDeletionCostController {
        reconcile_pod_deletion_cost_controller(state);
        return;
    }

    let nodes = active_nodes(state);
    if nodes.is_empty() {
        return;
    }

    // Partition into normal and protected (do-not-disrupt) groups
    let mut normal: Vec<(NodeId, Vec<PodId>)> = Vec::new();
    let mut protected: Vec<(NodeId, Vec<PodId>)> = Vec::new();
    for (nid, pods) in nodes {
        if has_do_not_disrupt(state, &pods) {
            protected.push((nid, pods));
        } else {
            normal.push((nid, pods));
        }
    }

    // Rank normal nodes by strategy, then protected nodes get higher costs
    rank_nodes(state, &mut normal, strategy);
    let normal_len = normal.len() as i32;

    // Assign costs: normal group first (lower costs = deleted first)
    for (i, (_, pod_ids)) in normal.iter().enumerate() {
        let cost = BASE_RANK + i as i32;
        for &pid in pod_ids {
            if let Some(pod) = state.pods.get_mut(pid) {
                pod.deletion_cost = Some(cost);
            }
        }
    }

    // Protected group gets higher costs (preserved during scale-down)
    for (i, (_, pod_ids)) in protected.iter().enumerate() {
        let cost = BASE_RANK + normal_len + i as i32;
        for &pid in pod_ids {
            if let Some(pod) = state.pods.get_mut(pid) {
                pod.deletion_cost = Some(cost);
            }
        }
    }
}

/// Maximum number of consolidation-candidate nodes annotated per cycle (PR #2894).
const TOP_N_CANDIDATES: usize = 50;

/// Models the pod-deletion-cost controller from Karpenter PR #2894.
///
/// Three-tier ranking:
///   Tier 0 (lowest cost): cordoned nodes (proxy for drifted — being consolidated)
///   Tier 1 (middle):      normal nodes
///   Tier 2 (highest cost): do-not-disrupt nodes
///
/// Within each tier, nodes are sorted by pod count ascending (fewest pods = lowest
/// deletion cost = deleted first by RS). Only the top 50 candidates get annotated.
fn reconcile_pod_deletion_cost_controller(state: &mut ClusterState) {
    let nodes = active_nodes(state);
    if nodes.is_empty() {
        return;
    }

    // Three-tier partition
    let mut drifted: Vec<(NodeId, Vec<PodId>)> = Vec::new();
    let mut normal: Vec<(NodeId, Vec<PodId>)> = Vec::new();
    let mut protected: Vec<(NodeId, Vec<PodId>)> = Vec::new();

    for (nid, pods) in nodes {
        let node = match state.nodes.get(nid) {
            Some(n) => n,
            None => continue,
        };
        if has_do_not_disrupt(state, &pods) || node.do_not_disrupt {
            protected.push((nid, pods));
        } else if node.cordoned {
            drifted.push((nid, pods));
        } else {
            normal.push((nid, pods));
        }
    }

    // Sort each tier by pod count ascending (fewest pods = best consolidation target)
    let sort_by_pod_count = |v: &mut Vec<(NodeId, Vec<PodId>)>| {
        v.sort_by_key(|(_, pods)| pods.len());
    };
    sort_by_pod_count(&mut drifted);
    sort_by_pod_count(&mut normal);
    sort_by_pod_count(&mut protected);

    // Chain tiers: drifted (lowest cost) → normal → protected (highest cost)
    // Only annotate the top TOP_N_CANDIDATES nodes
    let all: Vec<&(NodeId, Vec<PodId>)> = drifted.iter()
        .chain(normal.iter())
        .chain(protected.iter())
        .take(TOP_N_CANDIDATES)
        .collect();

    for (i, (_, pod_ids)) in all.iter().enumerate() {
        let cost = BASE_RANK + i as i32;
        for pid in pod_ids.iter().copied() {
            if let Some(pod) = state.pods.get_mut(pid) {
                pod.deletion_cost = Some(cost);
            }
        }
    }
}

/// Sort nodes in-place according to the ranking strategy.
/// After sorting, index 0 = most preferred for deletion (lowest cost).
fn rank_nodes(
    state: &ClusterState,
    nodes: &mut [(NodeId, Vec<PodId>)],
    strategy: DeletionCostStrategy,
) {
    match strategy {
        DeletionCostStrategy::None | DeletionCostStrategy::PodDeletionCostController => {}
        DeletionCostStrategy::Random => {
            // Deterministic "random" — use node id index as pseudo-random key
            // Real randomness would need an Rng, but for simulation reproducibility
            // we use a simple hash-like ordering.
            nodes.sort_by_key(|(nid, _)| nid.index);
        }
        DeletionCostStrategy::PreferEmptyingNodes => {
            // Fewest running pods first: nodes with fewer pods get lower
            // deletion costs so RS scale-down removes their pods first,
            // creating fully empty nodes for consolidation to terminate.
            nodes.sort_by_key(|(_, pods)| pods.len());
        }
        DeletionCostStrategy::LargestFirst => {
            // LargestToSmallest: rank by node allocatable CPU descending
            nodes.sort_by(|a, b| {
                let ca = state.nodes.get(a.0).map_or(0, |n| n.allocatable.cpu_millis);
                let cb = state.nodes.get(b.0).map_or(0, |n| n.allocatable.cpu_millis);
                cb.cmp(&ca)
            });
        }
        DeletionCostStrategy::UnallocatedVcpu => {
            // UnallocatedVCPUPerPodCost: rank by (unallocated_cpu / pod_count) descending
            // Targets inefficiently packed nodes.
            nodes.sort_by(|a, b| {
                let score = |nid: NodeId, pods: &[PodId]| -> u64 {
                    let n = match state.nodes.get(nid) {
                        Some(n) => n,
                        None => return 0,
                    };
                    let unalloc = n.allocatable.cpu_millis.saturating_sub(n.allocated.cpu_millis);
                    let count = pods.len().max(1) as u64;
                    unalloc / count
                };
                let sa = score(a.0, &a.1);
                let sb = score(b.0, &b.1);
                sb.cmp(&sa)
            });
        }
    }
}
