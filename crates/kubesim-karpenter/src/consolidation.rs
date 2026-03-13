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
use std::collections::HashSet;

use crate::nodepool::NodePool;
use crate::version::{ConsolidationStrategy, DisruptionReason, VersionProfile, evaluate_schedule};

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

/// Identify empty nodes (ready, not cordoned, zero non-daemonset pods, not do-not-disrupt) belonging to the given pool.
/// Nodes younger than `consolidate_after_ns` are exempt from consolidation.
fn find_empty_nodes(state: &ClusterState, pool_name: &str, consolidate_after_ns: u64) -> Vec<NodeId> {
    state
        .nodes
        .iter()
        .filter(|(_, n)| {
            n.conditions.ready && !n.cordoned && !n.do_not_disrupt
                && n.pool_name == pool_name
                && state.time.0 >= n.created_at.0.saturating_add(consolidate_after_ns)
                && n.pods.iter().all(|&pid| {
                    state.pods.get(pid).map_or(true, |p| p.is_daemonset)
                })
        })
        .map(|(id, _)| id)
        .collect()
}

/// Compute a multi-factor disruption score for a candidate node.
///
/// Upstream Karpenter sorts consolidation candidates by:
/// 1. Disruption cost — max pod priority + count of PDB-covered pods (lower = better candidate)
/// 2. Pod count — fewer pods means less disruption (lower = better)
/// 3. Node age — older nodes preferred for consolidation (lower created_at = better)
/// 4. Spot preference (v1.x) — on-demand nodes preferred over spot for consolidation
///    (spot nodes are already cheap; consolidating on-demand saves more)
///
/// Returns `(spot_penalty, disruption_cost, pod_count, negative_age)` for ascending sort.
fn candidate_score(state: &ClusterState, node: &Node) -> (u8, i64, usize, u64) {
    let mut max_priority: i32 = 0;
    let mut pdb_covered: i64 = 0;
    let mut non_ds_count: usize = 0;
    for &pid in &node.pods {
        if let Some(pod) = state.pods.get(pid) {
            if pod.is_daemonset { continue; }
            non_ds_count += 1;
            if pod.priority > max_priority {
                max_priority = pod.priority;
            }
            if state.pdbs.iter().any(|pdb| pod.labels.matches(&pdb.selector)) {
                pdb_covered += 1;
            }
        }
    }
    let disruption_cost = max_priority as i64 + pdb_covered;
    // Invert age so ascending sort prefers older nodes (lower created_at).
    let negative_age = node.created_at.0;
    // Spot nodes get penalty=1 so on-demand nodes (penalty=0) are consolidated first.
    let spot_penalty = match node.lifecycle {
        NodeLifecycle::Spot { .. } => 1u8,
        NodeLifecycle::OnDemand => 0u8,
    };
    (spot_penalty, disruption_cost, non_ds_count, negative_age)
}

/// Sort candidate nodes by multi-factor disruption score (ascending).
/// Candidates with lower disruption cost are consolidated first.
fn sort_candidates(state: &ClusterState, candidates: &mut [(NodeId, &Node)]) {
    candidates.sort_by(|a, b| {
        let sa = candidate_score(state, a.1);
        let sb = candidate_score(state, b.1);
        sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Check whether all pods on `candidate` can fit on other ready, non-cordoned nodes,
/// excluding any nodes already selected for removal in this consolidation round.
///
/// Uses a greedy first-fit approach: for each pod, find any other node with
/// enough available resources AND passing scheduling constraints (affinity,
/// taints, topology spread). Real Karpenter uses full scheduling simulation;
/// this approximates it by running the scheduler's filter plugins.
fn pods_can_reschedule(
    state: &ClusterState,
    candidate: NodeId,
    nodes_being_removed: &HashSet<NodeId>,
) -> Option<Vec<PodId>> {
    use kubesim_scheduler::{
        FilterResult, TaintToleration, NodeAffinity,
        InterPodAffinityFilter, PodTopologySpreadFilter, FilterPlugin,
    };

    let node = state.nodes.get(candidate)?;
    if node.pods.is_empty() {
        return Some(Vec::new());
    }

    // Use stack-allocated filter references to avoid heap allocation per call
    let f1 = TaintToleration;
    let f2 = NodeAffinity;
    let f3 = InterPodAffinityFilter;
    let f4 = PodTopologySpreadFilter;
    let filters: [&dyn FilterPlugin; 4] = [&f1, &f2, &f3, &f4];

    // Collect pods to move (exclude daemonset pods — they are non-evictable)
    let pod_ids: Vec<PodId> = node.pods.iter().copied().collect();
    let pods: Vec<(PodId, &Pod)> = pod_ids
        .iter()
        .filter_map(|&pid| state.pods.get(pid).map(|p| (pid, p)))
        .filter(|(_, p)| !p.is_daemonset)
        .collect();

    // Build available capacity on other nodes, excluding candidate and nodes already being removed
    let mut other_avail: Vec<(NodeId, Resources)> = state
        .nodes
        .iter()
        .filter(|(nid, n)| {
            *nid != candidate
                && !nodes_being_removed.contains(nid)
                && n.conditions.ready
                && !n.cordoned
        })
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

    Some(pods.iter().map(|&(pid, _)| pid).collect())
}

/// Returns true if the node itself or any non-daemonset pod on the node has `do_not_disrupt` set.
fn node_has_do_not_disrupt(state: &ClusterState, node: &Node) -> bool {
    node.do_not_disrupt || node.pods.iter().any(|&pid| {
        state.pods.get(pid).map_or(false, |p| p.do_not_disrupt && !p.is_daemonset)
    })
}

/// Find underutilized nodes whose pods can all be rescheduled elsewhere.
///
/// For `SingleNode` strategy: evaluates one candidate at a time, returns at most 1 action.
/// For `MultiNode` strategy: builds a candidate set greedily — each candidate is checked
/// against remaining capacity excluding nodes already selected for removal.
/// Nodes younger than `consolidate_after_ns` are exempt from consolidation.
///
/// `max_candidates` limits how many candidates are evaluated per pass (0 = unlimited).
/// `timeout_candidates` limits multi-node evaluation iterations before falling back
/// to single-node (0 = no timeout). Only applies when strategy is `MultiNode`.
fn find_underutilized_nodes(
    state: &ClusterState,
    pool_name: &str,
    strategy: ConsolidationStrategy,
    consolidate_after_ns: u64,
    max_candidates: usize,
    timeout_candidates: usize,
) -> Vec<ConsolidationAction> {
    let mut actions = Vec::new();
    let mut nodes_being_removed: HashSet<NodeId> = HashSet::new();

    let mut candidates: Vec<(NodeId, &Node)> = state
        .nodes
        .iter()
        .filter(|(_, n)| {
            n.conditions.ready && !n.cordoned && n.pool_name == pool_name
                && state.time.0 >= n.created_at.0.saturating_add(consolidate_after_ns)
                // Must have at least one non-daemonset pod (empty nodes handled by find_empty_nodes)
                && n.pods.iter().any(|&pid| {
                    state.pods.get(pid).map_or(false, |p| !p.is_daemonset)
                })
        })
        .filter(|(_, n)| !node_has_do_not_disrupt(state, n))
        .collect();
    sort_candidates(state, &mut candidates);

    // Apply batch size limit
    if max_candidates > 0 && candidates.len() > max_candidates {
        candidates.truncate(max_candidates);
    }

    let mut evaluated: usize = 0;
    let mut timed_out = false;

    for (nid, _) in &candidates {
        // Multi-node timeout: if we've evaluated enough candidates, fall back to single-node
        if strategy == ConsolidationStrategy::MultiNode && timeout_candidates > 0 && evaluated >= timeout_candidates {
            timed_out = true;
            break;
        }
        evaluated += 1;

        if let Some(pod_ids) = pods_can_reschedule(state, *nid, &nodes_being_removed) {
            nodes_being_removed.insert(*nid);
            actions.push(ConsolidationAction::DrainAndTerminate {
                node_id: *nid,
                pod_ids,
            });
            if strategy == ConsolidationStrategy::SingleNode {
                break;
            }
        }
    }

    // Timeout fallback: multi-node timed out, try single-node on remaining candidates
    if timed_out {
        for (nid, _) in candidates.iter().skip(evaluated) {
            if nodes_being_removed.contains(nid) {
                continue;
            }
            if let Some(pod_ids) = pods_can_reschedule(state, *nid, &nodes_being_removed) {
                nodes_being_removed.insert(*nid);
                actions.push(ConsolidationAction::DrainAndTerminate {
                    node_id: *nid,
                    pod_ids,
                });
                break; // single-node: at most 1 additional
            }
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
        .filter(|(_, n)| {
            n.conditions.ready && !n.cordoned && n.pool_name == pool.name
                && n.pods.iter().any(|&pid| state.pods.get(pid).map_or(false, |p| !p.is_daemonset))
        })
        .filter(|(_, n)| !node_has_do_not_disrupt(state, n))
        .collect();
    sort_candidates(state, &mut candidates);

    for (nid, node) in candidates {
        // Skip if pods can already be rescheduled (delete path handles these)
        if pods_can_reschedule(state, nid, &HashSet::new()).is_some() {
            continue;
        }

        // Compute total resource demand of non-daemonset pods on this node
        let mut total_cpu: u64 = 0;
        let mut total_mem: u64 = 0;
        let mut total_gpu: u32 = 0;
        let pod_ids: Vec<PodId> = node.pods.iter().copied()
            .filter(|&pid| !state.pods.get(pid).map_or(false, |p| p.is_daemonset))
            .collect();
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
    pool_name: &str,
) -> Vec<ConsolidationAction> {
    evaluate_versioned(state, policy, max_disrupted, None, None, pool_name, 0)
}

/// Version-aware consolidation evaluation.
/// When `profile` is `None`, uses v1.x default behavior.
/// When `catalog` is provided and replace_consolidation is enabled, evaluates
/// the replace path for nodes that can't be consolidated by deletion.
/// `consolidate_after_ns` exempts nodes younger than this duration from consolidation.
pub fn evaluate_versioned(
    state: &ClusterState,
    policy: ConsolidationPolicy,
    max_disrupted: u32,
    profile: Option<&VersionProfile>,
    catalog: Option<(&Catalog, &NodePool)>,
    pool_name: &str,
    consolidate_after_ns: u64,
) -> Vec<ConsolidationAction> {
    let strategy = profile
        .map(|p| p.consolidation_strategy)
        .unwrap_or(ConsolidationStrategy::MultiNode);
    let replace_enabled = profile
        .map(|p| p.replace_consolidation)
        .unwrap_or(true);
    let max_candidates = profile.map(|p| p.max_candidates).unwrap_or(10);
    let timeout_candidates = profile.map(|p| p.multi_node_timeout_candidates).unwrap_or(10);

    // v1.x per-reason budgets: compute per-reason caps from profile budgets.
    // If a budget entry has reasons, it applies only to those reasons.
    // If no reasons, it's a global fallback (same as v0.35 flat percentage).
    let total_nodes = state.nodes.len() as u32;
    let (empty_budget, underutilized_budget) = if let Some(p) = profile {
        let mut empty_cap = max_disrupted;
        let mut underutil_cap = max_disrupted;
        let has_per_reason = p.budgets.iter().any(|b| !b.reasons.is_empty());
        if has_per_reason {
            empty_cap = 0;
            underutil_cap = 0;
        }
        for b in &p.budgets {
            // Resolve effective percentage: if schedule is set, use active/inactive budget
            let effective_pct = match (&b.schedule, p.version) {
                (Some(sched), crate::version::KarpenterVersion::V1) => {
                    if evaluate_schedule(state.time, sched) {
                        b.active_budget.unwrap_or(b.max_percent)
                    } else {
                        b.inactive_budget.unwrap_or(b.max_percent)
                    }
                }
                _ => b.max_percent,
            };
            let cap = ((total_nodes as u64 * effective_pct as u64) / 100).max(1) as u32;
            if b.reasons.is_empty() {
                empty_cap = if has_per_reason { empty_cap.max(cap) } else { cap };
                underutil_cap = if has_per_reason { underutil_cap.max(cap) } else { cap };
            } else {
                for r in &b.reasons {
                    match r {
                        DisruptionReason::Empty => empty_cap = empty_cap.max(cap),
                        DisruptionReason::Underutilized => underutil_cap = underutil_cap.max(cap),
                        DisruptionReason::Drifted => {}
                    }
                }
            }
        }
        (empty_cap, underutil_cap)
    } else {
        (max_disrupted, max_disrupted)
    };

    let mut actions: Vec<ConsolidationAction> = Vec::new();
    let mut total_used: u32 = 0;

    // WhenEmpty always runs (both policies include it)
    let mut empty_used: u32 = 0;
    for nid in find_empty_nodes(state, pool_name, consolidate_after_ns) {
        if empty_used >= empty_budget || total_used >= max_disrupted {
            break;
        }
        actions.push(ConsolidationAction::TerminateEmpty(nid));
        empty_used += 1;
        total_used += 1;
    }

    if policy == ConsolidationPolicy::WhenUnderutilized && total_used < max_disrupted {
        let mut underutil_used: u32 = 0;
        for action in find_underutilized_nodes(state, pool_name, strategy, consolidate_after_ns, max_candidates, timeout_candidates) {
            if underutil_used >= underutilized_budget || total_used >= max_disrupted {
                break;
            }
            actions.push(action);
            underutil_used += 1;
            total_used += 1;
        }

        // Replace path (v1.x only)
        if total_used < max_disrupted && replace_enabled && strategy == ConsolidationStrategy::MultiNode {
            if let Some((cat, pool)) = catalog {
                for action in find_replace_candidates(state, cat, pool) {
                    if underutil_used >= underutilized_budget || total_used >= max_disrupted {
                        break;
                    }
                    actions.push(action);
                    underutil_used += 1;
                    total_used += 1;
                }
            }
        }
    }

    actions
}

/// Compute the max number of nodes that may be disrupted given the pool config
/// and current node count.
pub fn disruption_budget(pool: &NodePool, total_nodes: u32) -> u32 {
    if let Some(max_count) = pool.max_disrupted_count {
        max_count.min(total_nodes).max(1)
    } else {
        ((total_nodes as u64 * pool.max_disrupted_pct as u64) / 100).max(1) as u32
    }
}

// ── Scheduling simulation ───────────────────────────────────────

/// Validate a consolidation plan by simulating pod scheduling.
///
/// Collects all pods from candidate nodes, builds a virtual view of remaining
/// capacity (excluding candidate nodes), and checks that every displaced pod
/// can be placed on a remaining node. If any pod fails to schedule, shrinks
/// the candidate set by removing the last candidate and retries.
///
/// Returns the validated subset of actions (may be smaller than input).
fn simulate_and_validate(
    state: &ClusterState,
    actions: Vec<ConsolidationAction>,
) -> Vec<ConsolidationAction> {
    use kubesim_scheduler::{
        FilterResult, TaintToleration, NodeAffinity,
        InterPodAffinityFilter, PodTopologySpreadFilter, FilterPlugin,
    };

    if actions.is_empty() {
        return actions;
    }

    let f1 = TaintToleration;
    let f2 = NodeAffinity;
    let f3 = InterPodAffinityFilter;
    let f4 = PodTopologySpreadFilter;
    let filters: [&dyn FilterPlugin; 4] = [&f1, &f2, &f3, &f4];

    let mut validated = actions;

    loop {
        // Collect all candidate node IDs
        let candidate_nodes: HashSet<NodeId> = validated.iter().map(|a| match a {
            ConsolidationAction::TerminateEmpty(nid) => *nid,
            ConsolidationAction::DrainAndTerminate { node_id, .. } => *node_id,
            ConsolidationAction::Replace { node_id, .. } => *node_id,
        }).collect();

        // Collect all displaced pods
        let displaced_pods: Vec<PodId> = validated.iter().flat_map(|a| match a {
            ConsolidationAction::TerminateEmpty(_) => vec![],
            ConsolidationAction::DrainAndTerminate { pod_ids, .. } => pod_ids.clone(),
            ConsolidationAction::Replace { pod_ids, .. } => pod_ids.clone(),
        }).collect();

        if displaced_pods.is_empty() {
            break;
        }

        // Build virtual capacity on remaining nodes
        let mut remaining_avail: Vec<(NodeId, Resources)> = state
            .nodes
            .iter()
            .filter(|(nid, n)| {
                !candidate_nodes.contains(nid)
                    && n.conditions.ready
                    && !n.cordoned
            })
            .map(|(nid, n)| (nid, n.allocatable.saturating_sub(&n.allocated)))
            .collect();

        // Try to place every displaced pod
        let mut all_fit = true;
        for &pid in &displaced_pods {
            let pod = match state.pods.get(pid) {
                Some(p) => p,
                None => continue,
            };
            let slot = remaining_avail.iter_mut().find(|(nid, avail)| {
                if !pod.requests.fits_in(avail) {
                    return false;
                }
                let target = match state.nodes.get(*nid) {
                    Some(n) => n,
                    None => return false,
                };
                filters.iter().all(|f| matches!(f.filter(state, pod, target), FilterResult::Pass))
            });
            match slot {
                Some((_, avail)) => *avail = avail.saturating_sub(&pod.requests),
                None => { all_fit = false; break; }
            }
        }

        if all_fit {
            break;
        }

        // Shrink candidate set by removing the last action and retry
        validated.pop();
        if validated.is_empty() {
            break;
        }
    }

    validated
}

/// Pre-execution validation: re-check that candidate nodes are still valid
/// before executing the consolidation plan. Between planning and executing,
/// new pods may have landed or nodes may have been cordoned.
///
/// For each action, verify:
/// - The node still exists, is ready, and is not already cordoned/draining
/// - Empty nodes are still empty
/// - Underutilized nodes' pods still fit on remaining nodes
///
/// If any check fails, the action is dropped (abort and re-plan next loop).
fn validate_before_execute(
    state: &ClusterState,
    actions: Vec<ConsolidationAction>,
) -> Vec<ConsolidationAction> {
    let mut validated = Vec::new();
    let mut nodes_being_removed: HashSet<NodeId> = HashSet::new();

    for action in actions {
        let (nid, is_empty) = match &action {
            ConsolidationAction::TerminateEmpty(nid) => (*nid, true),
            ConsolidationAction::DrainAndTerminate { node_id, .. } => (*node_id, false),
            ConsolidationAction::Replace { node_id, .. } => (*node_id, false),
        };

        let node = match state.nodes.get(nid) {
            Some(n) if n.conditions.ready && !n.cordoned => n,
            _ => continue, // node gone, not ready, or already cordoned
        };

        if is_empty {
            if !node.pods.is_empty() {
                continue; // pods landed since planning
            }
        } else if pods_can_reschedule(state, nid, &nodes_being_removed).is_none() {
            continue; // pods no longer fit elsewhere
        }

        nodes_being_removed.insert(nid);
        validated.push(action);
    }

    validated
}

// ── EventHandler integration ────────────────────────────────────

/// Karpenter consolidation handler for the simulation engine.
///
/// Implements a two-phase approach matching real Karpenter:
/// - PLAN phase: evaluate candidates + validate via scheduling simulation
/// - EXECUTE phase: cordon nodes immediately, schedule `NodeDrained` events
///   that perform actual pod eviction (handled by [`DrainHandler`])
pub struct ConsolidationHandler {
    pub pool: NodePool,
    pub policy: ConsolidationPolicy,
    /// Interval (ns) between consolidation loops in WallClock mode.
    pub loop_interval_ns: u64,
    /// Version profile controlling consolidation strategy.
    pub version_profile: Option<VersionProfile>,
    /// EC2 catalog for replace-path instance selection.
    pub catalog: Option<Catalog>,
    /// Minimum node age (ns) before it becomes eligible for consolidation.
    /// Prevents thrashing: provision → immediately consolidate → provision again.
    pub consolidate_after_ns: u64,
}

impl ConsolidationHandler {
    pub fn new(pool: NodePool, policy: ConsolidationPolicy) -> Self {
        Self {
            pool,
            policy,
            loop_interval_ns: 30_000_000_000, // 30s default
            version_profile: None,
            catalog: None,
            consolidate_after_ns: 0,
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
        let actions = evaluate_versioned(state, self.policy, max_d, self.version_profile.as_ref(), catalog_ref, &self.pool.name, self.consolidate_after_ns);

        // PLAN phase: validate via scheduling simulation — shrink candidate set
        // until all displaced pods can be placed on remaining nodes.
        let actions = simulate_and_validate(state, actions);

        // Pre-execution validation: re-check that candidates are still valid
        // (not cordoned, still underutilized, pods still fit elsewhere).
        let actions = validate_before_execute(state, actions);

        let mut follow_ups = Vec::new();

        // EXECUTE phase: cordon candidates immediately, schedule staggered drain events.
        // Pod eviction is deferred to NodeDrained events (handled by DrainHandler).
        const STABILIZATION_NS: u64 = 15_000_000_000;
        let mut action_offset: u64 = 0;

        for action in actions {
            match action {
                ConsolidationAction::TerminateEmpty(nid) => {
                    if let Some(n) = state.nodes.get_mut(nid) {
                        n.cordoned = true;
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 1),
                        event: Event::NodeCordoned(nid),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 2),
                        event: Event::NodeTerminated(nid),
                    });
                    action_offset += STABILIZATION_NS;
                }
                ConsolidationAction::DrainAndTerminate { node_id, .. } => {
                    if let Some(n) = state.nodes.get_mut(node_id) {
                        n.cordoned = true;
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 1),
                        event: Event::NodeCordoned(node_id),
                    });
                    // Eviction deferred to DrainHandler
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 2),
                        event: Event::NodeDrained(node_id),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 3),
                        event: Event::NodeTerminated(node_id),
                    });
                    action_offset += STABILIZATION_NS;
                }
                ConsolidationAction::Replace { node_id, replacement_instance_type, .. } => {
                    if let Some(n) = state.nodes.get_mut(node_id) {
                        n.cordoned = true;
                    }
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 1),
                        event: Event::NodeCordoned(node_id),
                    });
                    // Eviction deferred to DrainHandler
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 2),
                        event: Event::NodeDrained(node_id),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 3),
                        event: Event::NodeTerminated(node_id),
                    });
                    follow_ups.push(ScheduledEvent {
                        time: SimTime(time.0 + action_offset + 4),
                        event: Event::NodeLaunching(NodeSpec {
                            instance_type: replacement_instance_type,
                            labels: kubesim_core::LabelSet(self.pool.labels.clone()),
                            taints: self.pool.taints.clone(),
                            pool_name: self.pool.name.clone(),
                            do_not_disrupt: self.pool.do_not_disrupt,
                        }),
                    });
                    action_offset += STABILIZATION_NS;
                }
            }
        }

        // Always re-schedule next consolidation loop
        follow_ups.push(ScheduledEvent {
            time: SimTime(time.0 + self.loop_interval_ns),
            event: Event::KarpenterConsolidationLoop,
        });

        follow_ups
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

// ── Drain handler ───────────────────────────────────────────────

/// Handles `NodeDrained` events by evicting all pods from the drained node
/// and triggering rescheduling via the provisioning loop and RS reconcile.
///
/// This separates the EXECUTE phase from the PLAN phase: the consolidation
/// handler schedules staggered `NodeDrained` events, and this handler performs
/// the actual pod eviction one node at a time — preventing cascade eviction.
///
/// After eviction, emits:
/// - `KarpenterProvisioningLoop` so the provisioner (and glue handler) can
///   place evicted pods onto existing nodes or launch new ones
/// - `ReplicaSetReconcile` for each affected owner so the RS controller can
///   detect actual < desired and create replacement pods if needed
pub struct DrainHandler;

impl EventHandler for DrainHandler {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        let Event::NodeDrained(node_id) = event else {
            return Vec::new();
        };

        let pod_ids: Vec<PodId> = match state.nodes.get(*node_id) {
            Some(n) => n.pods.clone().into_vec(),
            None => return Vec::new(),
        };

        // Collect unique owners before eviction (skip daemonset pods)
        let mut owners = Vec::new();
        for &pid in &pod_ids {
            if let Some(pod) = state.pods.get(pid) {
                if pod.is_daemonset { continue; }
                if !owners.contains(&pod.owner) {
                    owners.push(pod.owner);
                }
            }
        }

        for pid in pod_ids {
            if state.pods.get(pid).map_or(false, |p| p.is_daemonset) { continue; }
            state.evict_pod(pid);
        }

        let mut follow_ups = Vec::new();

        // Trigger RS reconcile for each affected owner so the controller
        // can create replacement pods if actual < desired
        for owner in owners {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + 1),
                event: Event::ReplicaSetReconcile(owner),
            });
        }

        // Trigger provisioning so evicted pods get rescheduled on remaining nodes
        if !state.pending_queue.is_empty() {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + 1),
                event: Event::KarpenterProvisioningLoop,
            });
        }

        follow_ups
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
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
            created_at: SimTime(0),
            pool_name: "default".into(),
            do_not_disrupt: false,
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
            do_not_disrupt: false,
            duration_ns: None,
            is_daemonset: false,
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
            max_disrupted_count: None,
            weight: 0,
            do_not_disrupt: false,
        }
    }

    #[test]
    fn when_empty_terminates_empty_nodes() {
        let mut state = ClusterState::new();
        state.add_node(test_node(4000, 8_000_000_000)); // empty node

        let actions = evaluate(&state, ConsolidationPolicy::WhenEmpty, 10, "default");
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], ConsolidationAction::TerminateEmpty(_)));
    }

    #[test]
    fn when_empty_skips_nodes_with_pods() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));
        state.bind_pod(pid, nid);

        let actions = evaluate(&state, ConsolidationPolicy::WhenEmpty, 10, "default");
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

        let actions = evaluate(&state, ConsolidationPolicy::WhenUnderutilized, 10, "default");
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
        let actions = evaluate(&state, ConsolidationPolicy::WhenEmpty, 1, "default");
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
    fn disruption_budget_max_count_overrides_percent() {
        let mut pool = test_pool();
        pool.max_disrupted_count = Some(3);
        // max_count=3 on 100 nodes → only 3 (not 10% = 10)
        assert_eq!(disruption_budget(&pool, 100), 3);
    }

    #[test]
    fn disruption_budget_max_percent_20() {
        let mut pool = test_pool();
        pool.max_disrupted_pct = 20;
        // max_percent=20 on 50 nodes → 10
        assert_eq!(disruption_budget(&pool, 50), 10);
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
        // Should have NodeCordoned + NodeTerminated + re-schedule for continued consolidation
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

    #[test]
    fn do_not_disrupt_pod_prevents_consolidation() {
        let mut state = ClusterState::new();
        // Node A: has a do-not-disrupt pod
        let na = state.add_node(Node { cost_per_hour: 0.1, ..test_node(4000, 8_000_000_000) });
        let mut pod = test_pod(500, 500_000_000);
        pod.do_not_disrupt = true;
        let pid = state.submit_pod(pod);
        state.bind_pod(pid, na);

        // Node B: has capacity to absorb A's pod
        state.add_node(test_node(4000, 8_000_000_000));

        let actions = evaluate(&state, ConsolidationPolicy::WhenUnderutilized, 10, "default");
        // Node A must NOT be a consolidation candidate
        let drains_na = actions.iter().any(|a| match a {
            ConsolidationAction::DrainAndTerminate { node_id, .. } => *node_id == na,
            ConsolidationAction::Replace { node_id, .. } => *node_id == na,
            _ => false,
        });
        assert!(!drains_na, "node with do-not-disrupt pod should not be consolidated");
    }
}
