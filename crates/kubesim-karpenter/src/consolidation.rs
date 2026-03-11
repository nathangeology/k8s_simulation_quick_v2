//! Karpenter Consolidation — WhenEmpty and WhenUnderutilized policies.
//!
//! Periodically evaluates nodes for consolidation opportunities:
//! - **WhenEmpty**: nodes with zero running pods are terminated immediately.
//! - **WhenUnderutilized**: nodes whose pods can all fit on other existing nodes
//!   are cordoned, drained, and terminated.

use kubesim_core::*;
use kubesim_engine::{Event, EventHandler, ScheduledEvent};

use crate::nodepool::NodePool;

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
/// enough available resources. This is a simplified model — real Karpenter
/// uses full scheduling simulation.
fn pods_can_reschedule(
    state: &ClusterState,
    candidate: NodeId,
) -> Option<Vec<PodId>> {
    let node = state.nodes.get(candidate)?;
    if node.pods.is_empty() {
        return Some(Vec::new());
    }

    // Collect pods to move
    let pod_ids: Vec<PodId> = node.pods.iter().copied().collect();
    let pods: Vec<(PodId, Resources)> = pod_ids
        .iter()
        .filter_map(|&pid| state.pods.get(pid).map(|p| (pid, p.requests)))
        .collect();

    // Build available capacity on other nodes (mutable copy for greedy allocation)
    let mut other_avail: Vec<(NodeId, Resources)> = state
        .nodes
        .iter()
        .filter(|(nid, n)| *nid != candidate && n.conditions.ready && !n.cordoned)
        .map(|(nid, n)| (nid, n.allocatable.saturating_sub(&n.allocated)))
        .collect();

    for &(_, req) in &pods {
        let slot = other_avail
            .iter_mut()
            .find(|(_, avail)| req.fits_in(avail));
        match slot {
            Some((_, avail)) => *avail = avail.saturating_sub(&req),
            None => return None, // no room for this pod
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

/// Run one consolidation evaluation, returning actions to take.
/// Respects the disruption budget: at most `max_disrupted` nodes may be
/// disrupted in a single pass.
pub fn evaluate(
    state: &ClusterState,
    policy: ConsolidationPolicy,
    max_disrupted: u32,
) -> Vec<ConsolidationAction> {
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
        for action in find_underutilized_nodes(state) {
            if budget == 0 {
                break;
            }
            actions.push(action);
            budget -= 1;
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
}

impl ConsolidationHandler {
    pub fn new(pool: NodePool, policy: ConsolidationPolicy) -> Self {
        Self {
            pool,
            policy,
            loop_interval_ns: 30_000_000_000, // 30s default
        }
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
        let actions = evaluate(state, self.policy, max_d);
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
