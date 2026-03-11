//! KubeSim Scheduler — Filter and Score plugin chain modelling kube-scheduler.
//!
//! Implements the two-phase scheduling pipeline:
//! 1. **Filter**: eliminate nodes that cannot run the pod
//! 2. **Score**: rank remaining nodes, pick the highest
//!
//! Built-in plugins:
//! - Filters: `NodeResourcesFit`, `TaintToleration`, `NodeAffinity`, `InterPodAffinity`, `PodTopologySpreadFilter`
//! - Scorers: `MostAllocated`, `LeastAllocated`, `BalancedAllocation`, `NodeAffinityScore`, `InterPodAffinity`, `PodTopologySpreadScore`

pub use kubesim_core;

use kubesim_core::{
    AffinityType, ClusterState, Node, NodeId, Pod, PodAffinityTerm, PodId, PodPhase, Resources,
    Taint, WhenUnsatisfiable,
};
use std::collections::HashMap;

// ── Plugin traits ───────────────────────────────────────────────

/// Result of a filter plugin evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterResult {
    /// Node passes this filter.
    Pass,
    /// Node is rejected with a reason.
    Reject(String),
}

/// A filter plugin eliminates nodes that cannot run a pod.
pub trait FilterPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn filter(&self, state: &ClusterState, pod: &Pod, node: &Node) -> FilterResult;
}

/// A score plugin ranks feasible nodes. Higher score = more preferred.
pub trait ScorePlugin: Send + Sync {
    fn name(&self) -> &str;
    fn score(&self, state: &ClusterState, pod: &Pod, node: &Node) -> i64;
    fn weight(&self) -> i64;
}

// ── Built-in filters ────────────────────────────────────────────

/// Rejects nodes that lack sufficient allocatable resources for the pod's requests.
pub struct NodeResourcesFit;

impl FilterPlugin for NodeResourcesFit {
    fn name(&self) -> &str { "NodeResourcesFit" }

    fn filter(&self, state: &ClusterState, pod: &Pod, _node: &Node) -> FilterResult {
        // We need the node's handle to compute available resources, but the trait
        // receives &Node directly. Compute inline from the node's own fields.
        let _ = state;
        let available = _node.allocatable.saturating_sub(&_node.allocated);
        if pod.requests.fits_in(&available) {
            FilterResult::Pass
        } else {
            FilterResult::Reject("insufficient resources".into())
        }
    }
}

/// Rejects nodes whose taints are not tolerated by the pod.
pub struct TaintToleration;

impl FilterPlugin for TaintToleration {
    fn name(&self) -> &str { "TaintToleration" }

    fn filter(&self, _state: &ClusterState, pod: &Pod, node: &Node) -> FilterResult {
        for taint in &node.taints {
            if !is_tolerated(taint, &pod.scheduling_constraints.tolerations) {
                return FilterResult::Reject(format!("taint {} not tolerated", taint.key));
            }
        }
        FilterResult::Pass
    }
}

fn is_tolerated(taint: &Taint, tolerations: &[kubesim_core::Toleration]) -> bool {
    tolerations.iter().any(|t| t.tolerates(taint))
}

/// Rejects nodes that don't satisfy required node affinity terms.
pub struct NodeAffinity;

impl FilterPlugin for NodeAffinity {
    fn name(&self) -> &str { "NodeAffinity" }

    fn filter(&self, _state: &ClusterState, pod: &Pod, node: &Node) -> FilterResult {
        for term in &pod.scheduling_constraints.node_affinity {
            if let AffinityType::Required = term.affinity_type {
                let all_match = term.match_labels.0.iter().all(|(k, v)| node.labels.get(k) == Some(v.as_str()));
                if !all_match {
                    return FilterResult::Reject("node affinity required term not matched".into());
                }
            }
        }
        FilterResult::Pass
    }
}

// ── Built-in scorers ────────────────────────────────────────────

/// Favours nodes with higher utilisation (bin-packing). Score 0–100.
pub struct MostAllocated {
    pub weight: i64,
}

impl MostAllocated {
    pub fn new(weight: i64) -> Self { Self { weight } }
}

impl ScorePlugin for MostAllocated {
    fn name(&self) -> &str { "MostAllocated" }

    fn score(&self, _state: &ClusterState, pod: &Pod, node: &Node) -> i64 {
        utilisation_score(&node.allocated.saturating_add(&pod.requests), &node.allocatable)
    }

    fn weight(&self) -> i64 { self.weight }
}

/// Favours nodes with lower utilisation (spreading). Score 0–100.
pub struct LeastAllocated {
    pub weight: i64,
}

impl LeastAllocated {
    pub fn new(weight: i64) -> Self { Self { weight } }
}

impl ScorePlugin for LeastAllocated {
    fn name(&self) -> &str { "LeastAllocated" }

    fn score(&self, _state: &ClusterState, pod: &Pod, node: &Node) -> i64 {
        100 - utilisation_score(&node.allocated.saturating_add(&pod.requests), &node.allocatable)
    }

    fn weight(&self) -> i64 { self.weight }
}

/// Scores nodes based on preferred node affinity terms. Each matching preferred
/// term adds its weight to the score.
pub struct NodeAffinityScore;

impl ScorePlugin for NodeAffinityScore {
    fn name(&self) -> &str { "NodeAffinityScore" }

    fn score(&self, _state: &ClusterState, pod: &Pod, node: &Node) -> i64 {
        let mut total: i64 = 0;
        for term in &pod.scheduling_constraints.node_affinity {
            if let AffinityType::Preferred { weight } = term.affinity_type {
                let all_match = term.match_labels.0.iter().all(|(k, v)| node.labels.get(k) == Some(v.as_str()));
                if all_match {
                    total += weight as i64;
                }
            }
        }
        total
    }

    fn weight(&self) -> i64 { 1 }
}

/// Penalises nodes with imbalanced CPU vs memory utilisation.
/// Score 0–100: 100 = perfectly balanced, 0 = maximally imbalanced.
/// Mirrors upstream `BalancedAllocation` from `noderesources/balanced_allocation.go`.
pub struct BalancedAllocation {
    pub weight: i64,
}

impl BalancedAllocation {
    pub fn new(weight: i64) -> Self { Self { weight } }
}

impl ScorePlugin for BalancedAllocation {
    fn name(&self) -> &str { "BalancedAllocation" }

    fn score(&self, _state: &ClusterState, pod: &Pod, node: &Node) -> i64 {
        let used = node.allocated.saturating_add(&pod.requests);
        let cpu_frac = if node.allocatable.cpu_millis > 0 {
            used.cpu_millis as f64 / node.allocatable.cpu_millis as f64
        } else { 0.0 };
        let mem_frac = if node.allocatable.memory_bytes > 0 {
            used.memory_bytes as f64 / node.allocatable.memory_bytes as f64
        } else { 0.0 };
        let diff = (cpu_frac - mem_frac).abs();
        ((1.0 - diff) * 100.0).max(0.0) as i64
    }

    fn weight(&self) -> i64 { self.weight }
}

/// Average utilisation percentage across CPU and memory (0–100).
fn utilisation_score(used: &Resources, capacity: &Resources) -> i64 {
    let cpu_pct = if capacity.cpu_millis > 0 {
        (used.cpu_millis as f64 / capacity.cpu_millis as f64 * 100.0) as i64
    } else {
        0
    };
    let mem_pct = if capacity.memory_bytes > 0 {
        (used.memory_bytes as f64 / capacity.memory_bytes as f64 * 100.0) as i64
    } else {
        0
    };
    (cpu_pct + mem_pct) / 2
}

// ── InterPodAffinity helpers ────────────────────────────────────

/// Returns the topology value for a node given a topology key, if present.
fn node_topology_value<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.labels.get(key)
}

/// Check whether any running pod on nodes sharing the same topology domain
/// matches the given label selector.
fn topology_has_matching_pod(
    state: &ClusterState,
    topology_key: &str,
    topology_value: &str,
    term: &PodAffinityTerm,
) -> bool {
    for (_nid, node) in state.nodes.iter() {
        if node_topology_value(node, topology_key) != Some(topology_value) {
            continue;
        }
        for &pid in &node.pods {
            if let Some(p) = state.pods.get(pid) {
                if p.labels.matches(&term.label_selector) {
                    return true;
                }
            }
        }
    }
    false
}

/// Count matching pods in a topology domain.
fn count_matching_pods_in_domain(
    state: &ClusterState,
    topology_key: &str,
    topology_value: &str,
    term: &PodAffinityTerm,
) -> i64 {
    let mut count = 0i64;
    for (_nid, node) in state.nodes.iter() {
        if node_topology_value(node, topology_key) != Some(topology_value) {
            continue;
        }
        for &pid in &node.pods {
            if let Some(p) = state.pods.get(pid) {
                if p.labels.matches(&term.label_selector) {
                    count += 1;
                }
            }
        }
    }
    count
}

// ── InterPodAffinity filter ─────────────────────────────────────

/// Filter: rejects nodes where required pod affinity/anti-affinity is violated.
///
/// For required affinity: the candidate node's topology domain must contain
/// at least one matching pod.
/// For required anti-affinity: the candidate node's topology domain must NOT
/// contain any matching pod.
pub struct InterPodAffinityFilter;

impl FilterPlugin for InterPodAffinityFilter {
    fn name(&self) -> &str { "InterPodAffinity" }

    fn filter(&self, state: &ClusterState, pod: &Pod, node: &Node) -> FilterResult {
        for term in &pod.scheduling_constraints.pod_affinity {
            if !matches!(term.affinity_type, AffinityType::Required) {
                continue;
            }
            let topo_val = match node_topology_value(node, &term.topology_key) {
                Some(v) => v,
                None => {
                    // Node lacks the topology key — cannot satisfy affinity,
                    // and anti-affinity is vacuously satisfied.
                    if term.anti {
                        continue;
                    } else {
                        return FilterResult::Reject(format!(
                            "node missing topology key {}",
                            term.topology_key
                        ));
                    }
                }
            };
            let has_match =
                topology_has_matching_pod(state, &term.topology_key, topo_val, term);
            if term.anti && has_match {
                return FilterResult::Reject(format!(
                    "anti-affinity violated in topology {}={}",
                    term.topology_key, topo_val
                ));
            }
            if !term.anti && !has_match {
                return FilterResult::Reject(format!(
                    "affinity unsatisfied in topology {}={}",
                    term.topology_key, topo_val
                ));
            }
        }
        FilterResult::Pass
    }
}

// ── InterPodAffinity scorer ─────────────────────────────────────

/// Score: weighted preference based on matching pod distribution in topology domains.
///
/// For preferred affinity: higher score when more matching pods share the domain.
/// For preferred anti-affinity: higher score when fewer matching pods share the domain.
pub struct InterPodAffinityScore {
    pub weight: i64,
}

impl InterPodAffinityScore {
    pub fn new(weight: i64) -> Self { Self { weight } }
}

impl ScorePlugin for InterPodAffinityScore {
    fn name(&self) -> &str { "InterPodAffinity" }

    fn score(&self, state: &ClusterState, pod: &Pod, node: &Node) -> i64 {
        let mut total = 0i64;
        for term in &pod.scheduling_constraints.pod_affinity {
            let term_weight = match term.affinity_type {
                AffinityType::Preferred { weight } => weight as i64,
                AffinityType::Required => continue,
            };
            let topo_val = match node_topology_value(node, &term.topology_key) {
                Some(v) => v,
                None => continue,
            };
            let count =
                count_matching_pods_in_domain(state, &term.topology_key, topo_val, term);
            if term.anti {
                // Fewer matching pods → higher score
                total -= count * term_weight;
            } else {
                // More matching pods → higher score
                total += count * term_weight;
            }
        }
        total
    }

    fn weight(&self) -> i64 { self.weight }
}


// ── Topology spread helpers ─────────────────────────────────────

/// Count matching pods per topology domain for a given topology key and label selector.
fn domain_counts(state: &ClusterState, topology_key: &str, selector: &kubesim_core::LabelSelector) -> HashMap<String, i32> {
    let mut counts: HashMap<String, i32> = HashMap::new();
    for (_nid, node) in state.nodes.iter() {
        if let Some(domain) = node.labels.get(topology_key) {
            counts.entry(domain.to_string()).or_insert(0);
        }
    }
    for (_pid, pod) in state.pods.iter() {
        if pod.phase != kubesim_core::PodPhase::Running { continue; }
        if !pod.labels.matches(selector) { continue; }
        if let Some(node_id) = pod.node {
            if let Some(node) = state.nodes.get(node_id) {
                if let Some(domain) = node.labels.get(topology_key) {
                    *counts.entry(domain.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    counts
}

// ── PodTopologySpread filter ────────────────────────────────────

pub struct PodTopologySpreadFilter;

impl FilterPlugin for PodTopologySpreadFilter {
    fn name(&self) -> &str { "PodTopologySpreadFilter" }

    fn filter(&self, state: &ClusterState, pod: &Pod, node: &Node) -> FilterResult {
        for constraint in &pod.scheduling_constraints.topology_spread {
            if constraint.when_unsatisfiable != WhenUnsatisfiable::DoNotSchedule { continue; }
            let domain = match node.labels.get(&constraint.topology_key) {
                Some(d) => d.to_string(),
                None => return FilterResult::Reject(format!("node missing topology key {}", constraint.topology_key)),
            };
            let counts = domain_counts(state, &constraint.topology_key, &constraint.label_selector);
            let min_count = counts.values().copied().min().unwrap_or(0);
            let my_count = counts.get(&domain).copied().unwrap_or(0);
            let new_count = my_count + 1;
            let new_min = if my_count == min_count {
                counts.values().copied().map(|c| if c == my_count { new_count.min(c) } else { c }).min().unwrap_or(new_count)
            } else { min_count };
            let skew = new_count - new_min;
            if skew > constraint.max_skew as i32 {
                return FilterResult::Reject(format!("topology {} skew {} exceeds maxSkew {}", constraint.topology_key, skew, constraint.max_skew));
            }
        }
        FilterResult::Pass
    }
}

// ── PodTopologySpread scorer ────────────────────────────────────

pub struct PodTopologySpreadScore { pub weight: i64 }

impl PodTopologySpreadScore {
    pub fn new(weight: i64) -> Self { Self { weight } }
}

impl ScorePlugin for PodTopologySpreadScore {
    fn name(&self) -> &str { "PodTopologySpreadScore" }

    fn score(&self, state: &ClusterState, pod: &Pod, node: &Node) -> i64 {
        let mut total_skew: i32 = 0;
        let mut num_constraints: i32 = 0;
        for constraint in &pod.scheduling_constraints.topology_spread {
            if constraint.when_unsatisfiable != WhenUnsatisfiable::ScheduleAnyway { continue; }
            let domain = match node.labels.get(&constraint.topology_key) {
                Some(d) => d.to_string(),
                None => continue,
            };
            let counts = domain_counts(state, &constraint.topology_key, &constraint.label_selector);
            let min_count = counts.values().copied().min().unwrap_or(0);
            let my_count = counts.get(&domain).copied().unwrap_or(0) + 1;
            total_skew += (my_count - min_count).max(0);
            num_constraints += 1;
        }
        if num_constraints == 0 { return 0; }
        let avg_skew = total_skew as f64 / num_constraints as f64;
        (100.0 - avg_skew.min(100.0)) as i64
    }

    fn weight(&self) -> i64 { self.weight }
}
// ── Scheduler profile ───────────────────────────────────────────

/// A named scheduler profile with configurable filter and score plugins.
pub struct SchedulerProfile {
    pub name: String,
    pub filters: Vec<Box<dyn FilterPlugin>>,
    pub scorers: Vec<Box<dyn ScorePlugin>>,
}

impl SchedulerProfile {
    /// Create a profile with the default filter chain and the given scoring strategy.
    pub fn with_scoring(name: impl Into<String>, scoring: ScoringStrategy) -> Self {
        let scorer: Box<dyn ScorePlugin> = match scoring {
            ScoringStrategy::MostAllocated => Box::new(MostAllocated::new(1)),
            ScoringStrategy::LeastAllocated => Box::new(LeastAllocated::new(1)),
        };
        Self {
            name: name.into(),
            filters: vec![
                Box::new(NodeResourcesFit),
                Box::new(TaintToleration),
                Box::new(NodeAffinity),
                Box::new(InterPodAffinityFilter),
                Box::new(PodTopologySpreadFilter),
            ],
            scorers: vec![scorer, Box::new(BalancedAllocation::new(1)), Box::new(NodeAffinityScore), Box::new(InterPodAffinityScore::new(1)), Box::new(PodTopologySpreadScore::new(1))],
        }
    }
}

/// High-level scoring strategy selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoringStrategy {
    MostAllocated,
    LeastAllocated,
}

// ── Preemption ──────────────────────────────────────────────────

/// A preemption candidate: a node and the set of victims to evict.
#[derive(Debug, Clone)]
pub struct PreemptionCandidate {
    pub node_id: NodeId,
    pub victims: Vec<PodId>,
    pub num_pdb_violations: u32,
}

/// Result of a preemption evaluation.
#[derive(Debug)]
pub enum PreemptResult {
    /// Preemption found: evict victims, then bind preemptor to node.
    Preempt(PreemptionCandidate),
    /// No viable preemption candidate.
    NoCandidate,
}

/// Count PDB violations for a proposed victim set.
fn count_pdb_violations(state: &ClusterState, victims: &[PodId]) -> u32 {
    let mut violations = 0u32;
    for pdb in &state.pdbs {
        // Count currently matching running pods
        let total_matching = state.pods.iter()
            .filter(|(_, p)| p.phase == PodPhase::Running && p.labels.matches(&pdb.selector))
            .count() as u32;
        // Count victims matching this PDB
        let victim_matching = victims.iter()
            .filter(|vid| state.pods.get(**vid).map_or(false, |p| p.labels.matches(&pdb.selector)))
            .count() as u32;
        let remaining = total_matching.saturating_sub(victim_matching);
        if remaining < pdb.min_available {
            violations += 1;
        }
    }
    violations
}

/// Find the minimal victim set on a node: lower-priority pods whose removal
/// frees enough resources for the preemptor.
fn find_victims(state: &ClusterState, pod: &Pod, node: &Node, _node_id: NodeId) -> Option<Vec<PodId>> {
    // Collect lower-priority pods on this node, sorted by priority ascending (evict lowest first)
    let mut candidates: Vec<(PodId, i32, Resources)> = node.pods.iter()
        .filter_map(|&pid| {
            let p = state.pods.get(pid)?;
            if p.priority < pod.priority {
                Some((pid, p.priority, p.requests))
            } else {
                None
            }
        })
        .collect();
    candidates.sort_by_key(|&(_, pri, _)| pri);

    let needed = pod.requests;
    let available = node.allocatable.saturating_sub(&node.allocated);
    if needed.fits_in(&available) {
        return Some(vec![]); // Already fits, no victims needed
    }

    let mut freed = available;
    let mut victims = Vec::new();
    for (pid, _, res) in &candidates {
        victims.push(*pid);
        freed = freed.saturating_add(res);
        if needed.fits_in(&freed) {
            return Some(victims);
        }
    }
    None // Even evicting all lower-priority pods isn't enough
}

/// Evaluate preemption for a pod across all nodes. Returns the best candidate
/// minimizing: (1) PDB violations, (2) number of victims, (3) total victim priority.
fn evaluate_preemption(
    state: &ClusterState,
    pod: &Pod,
    filters: &[Box<dyn FilterPlugin>],
) -> PreemptResult {
    let mut best: Option<PreemptionCandidate> = None;

    for (nid, node) in state.nodes.iter() {
        if !node.conditions.ready || node.cordoned {
            continue;
        }
        // Check non-resource filters (skip NodeResourcesFit since preemption changes resources)
        let mut passes_other_filters = true;
        for filter in filters {
            if filter.name() == "NodeResourcesFit" {
                continue;
            }
            if let FilterResult::Reject(_) = filter.filter(state, pod, node) {
                passes_other_filters = false;
                break;
            }
        }
        if !passes_other_filters {
            continue;
        }

        let victims = match find_victims(state, pod, node, nid) {
            Some(v) => v,
            None => continue,
        };

        let pdb_violations = count_pdb_violations(state, &victims);
        let num_victims = victims.len() as u32;
        let total_priority: i64 = victims.iter()
            .filter_map(|vid| state.pods.get(*vid).map(|p| p.priority as i64))
            .sum();

        let is_better = match &best {
            None => true,
            Some(b) => {
                let b_total_pri: i64 = b.victims.iter()
                    .filter_map(|vid| state.pods.get(*vid).map(|p| p.priority as i64))
                    .sum();
                (pdb_violations, num_victims, total_priority) < (b.num_pdb_violations, b.victims.len() as u32, b_total_pri)
            }
        };
        if is_better {
            best = Some(PreemptionCandidate { node_id: nid, victims, num_pdb_violations: pdb_violations });
        }
    }

    match best {
        Some(c) => PreemptResult::Preempt(c),
        None => PreemptResult::NoCandidate,
    }
}

// ── Scheduler ───────────────────────────────────────────────────

/// The result of scheduling a single pod.
#[derive(Debug)]
pub enum ScheduleResult {
    /// Pod was bound to this node.
    Bound(NodeId),
    /// Pod was bound after preempting lower-priority victims.
    Preempted {
        node_id: NodeId,
        victims: Vec<PodId>,
    },
    /// No feasible node found.
    Unschedulable(Vec<String>),
}

/// kube-scheduler model: runs filter → score → select for pending pods.
pub struct Scheduler {
    pub profile: SchedulerProfile,
}

impl Scheduler {
    pub fn new(profile: SchedulerProfile) -> Self {
        Self { profile }
    }

    /// Attempt to schedule a single pod. Returns the chosen node or failure reasons.
    pub fn schedule_one(&self, state: &ClusterState, pod_id: PodId) -> ScheduleResult {
        let pod = match state.pods.get(pod_id) {
            Some(p) => p,
            None => return ScheduleResult::Unschedulable(vec!["pod not found".into()]),
        };

        // Filter phase
        let mut feasible: Vec<(NodeId, &Node)> = Vec::new();
        let mut reasons: Vec<String> = Vec::new();

        for (nid, node) in state.nodes.iter() {
            if !node.conditions.ready || node.cordoned {
                continue;
            }
            let mut passed = true;
            for filter in &self.profile.filters {
                if let FilterResult::Reject(reason) = filter.filter(state, pod, node) {
                    reasons.push(format!("{}: {}", filter.name(), reason));
                    passed = false;
                    break;
                }
            }
            if passed {
                feasible.push((nid, node));
            }
        }

        if feasible.is_empty() {
            // Attempt preemption
            match evaluate_preemption(state, pod, &self.profile.filters) {
                PreemptResult::Preempt(candidate) => {
                    return ScheduleResult::Preempted {
                        node_id: candidate.node_id,
                        victims: candidate.victims,
                    };
                }
                PreemptResult::NoCandidate => {
                    return ScheduleResult::Unschedulable(reasons);
                }
            }
        }

        // Score phase — normalize each scorer's output to [0,100] before weighting.
        // This matches upstream kube-scheduler's NormalizeScore extension point:
        // map [min, max] → [0, 100] via min-max scaling.
        let mut node_totals: Vec<(NodeId, i64)> = feasible.iter().map(|&(nid, _)| (nid, 0i64)).collect();

        for scorer in &self.profile.scorers {
            let raw: Vec<i64> = feasible.iter().map(|&(_, node)| scorer.score(state, pod, node)).collect();
            let min = raw.iter().copied().min().unwrap_or(0);
            let max = raw.iter().copied().max().unwrap_or(0);
            let range = max - min;
            for (i, &raw_val) in raw.iter().enumerate() {
                let normalized = if range > 0 { (raw_val - min) * 100 / range } else { 100 };
                node_totals[i].1 += normalized * scorer.weight();
            }
        }

        let best = node_totals.iter().max_by_key(|&&(_, score)| score).map(|&(nid, _)| nid).unwrap();

        ScheduleResult::Bound(best)
    }

    /// Schedule all pending pods in priority order. Returns (bound, unschedulable) counts.
    pub fn schedule_pending(&self, state: &mut ClusterState) -> (u32, u32) {
        let mut queue: Vec<PodId> = state.pending_queue.clone();
        // Sort by priority descending
        queue.sort_by(|a, b| {
            let pa = state.pods.get(*a).map(|p| p.priority).unwrap_or(0);
            let pb = state.pods.get(*b).map(|p| p.priority).unwrap_or(0);
            pb.cmp(&pa)
        });

        let mut bound = 0u32;
        let mut unschedulable = 0u32;

        for pod_id in queue {
            match self.schedule_one(state, pod_id) {
                ScheduleResult::Bound(node_id) => {
                    state.bind_pod(pod_id, node_id);
                    bound += 1;
                }
                ScheduleResult::Preempted { node_id, victims } => {
                    for vid in &victims {
                        state.evict_pod(*vid);
                    }
                    state.bind_pod(pod_id, node_id);
                    bound += 1;
                }
                ScheduleResult::Unschedulable(_) => {
                    unschedulable += 1;
                }
            }
        }

        (bound, unschedulable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubesim_core::*;

    fn ready_node(cpu: u64, mem: u64) -> Node {
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
        }
    }

    fn simple_pod(cpu: u64, mem: u64) -> Pod {
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

    #[test]
    fn schedule_binds_pod_to_feasible_node() {
        let mut state = ClusterState::new();
        state.add_node(ready_node(4000, 8_000_000_000));
        let pod_id = state.submit_pod(simple_pod(1000, 1_000_000_000));

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, unsched) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 1);
        assert_eq!(unsched, 0);
        assert_eq!(state.pods.get(pod_id).unwrap().phase, PodPhase::Running);
    }

    #[test]
    fn schedule_rejects_when_no_capacity() {
        let mut state = ClusterState::new();
        state.add_node(ready_node(1000, 1_000_000_000));
        let pod_id = state.submit_pod(simple_pod(2000, 1_000_000_000));

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, unsched) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 0);
        assert_eq!(unsched, 1);
        assert_eq!(state.pods.get(pod_id).unwrap().phase, PodPhase::Pending);
    }

    #[test]
    fn most_allocated_prefers_fuller_node() {
        let mut state = ClusterState::new();
        // Node A: 75% used
        let na = state.add_node(Node {
            allocated: Resources { cpu_millis: 3000, memory_bytes: 6_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            ..ready_node(4000, 8_000_000_000)
        });
        // Node B: 25% used
        let _nb = state.add_node(Node {
            allocated: Resources { cpu_millis: 1000, memory_bytes: 2_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            ..ready_node(4000, 8_000_000_000)
        });
        let pod_id = state.submit_pod(simple_pod(500, 500_000_000));

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::MostAllocated));
        let (bound, _) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 1);
        assert_eq!(state.pods.get(pod_id).unwrap().node, Some(na));
    }

    #[test]
    fn least_allocated_prefers_emptier_node() {
        let mut state = ClusterState::new();
        // Node A: 75% used
        let _na = state.add_node(Node {
            allocated: Resources { cpu_millis: 3000, memory_bytes: 6_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            ..ready_node(4000, 8_000_000_000)
        });
        // Node B: 25% used
        let nb = state.add_node(Node {
            allocated: Resources { cpu_millis: 1000, memory_bytes: 2_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            ..ready_node(4000, 8_000_000_000)
        });
        let pod_id = state.submit_pod(simple_pod(500, 500_000_000));

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, _) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 1);
        assert_eq!(state.pods.get(pod_id).unwrap().node, Some(nb));
    }

    #[test]
    fn taint_toleration_rejects_untolerated() {
        let mut state = ClusterState::new();
        let mut node = ready_node(4000, 8_000_000_000);
        node.taints.push(Taint {
            key: "dedicated".into(),
            value: "gpu".into(),
            effect: TaintEffect::NoSchedule,
        });
        state.add_node(node);
        state.submit_pod(simple_pod(1000, 1_000_000_000));

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, unsched) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 0);
        assert_eq!(unsched, 1);
    }

    #[test]
    fn taint_toleration_allows_tolerated() {
        let mut state = ClusterState::new();
        let mut node = ready_node(4000, 8_000_000_000);
        node.taints.push(Taint {
            key: "dedicated".into(),
            value: "gpu".into(),
            effect: TaintEffect::NoSchedule,
        });
        state.add_node(node);

        let mut pod = simple_pod(1000, 1_000_000_000);
        pod.scheduling_constraints.tolerations.push(Toleration {
            key: "dedicated".into(),
            operator: TolerationOperator::Equal,
            value: "gpu".into(),
            effect: Some(TaintEffect::NoSchedule),
        });
        state.submit_pod(pod);

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, _) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 1);
    }

    #[test]
    fn priority_ordering() {
        let mut state = ClusterState::new();
        // Only room for one pod
        state.add_node(ready_node(1000, 1_000_000_000));

        let mut low = simple_pod(1000, 1_000_000_000);
        low.priority = 0;
        let low_id = state.submit_pod(low);

        let mut high = simple_pod(1000, 1_000_000_000);
        high.priority = 100;
        let high_id = state.submit_pod(high);

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, unsched) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 1);
        assert_eq!(unsched, 1);
        // High priority pod should be scheduled
        assert_eq!(state.pods.get(high_id).unwrap().phase, PodPhase::Running);
        assert_eq!(state.pods.get(low_id).unwrap().phase, PodPhase::Pending);
    }

    #[test]
    fn not_ready_nodes_skipped() {
        let mut state = ClusterState::new();
        let mut node = ready_node(4000, 8_000_000_000);
        node.conditions.ready = false;
        state.add_node(node);
        state.submit_pod(simple_pod(1000, 1_000_000_000));

        let sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
        let (bound, unsched) = sched.schedule_pending(&mut state);
        assert_eq!(bound, 0);
        assert_eq!(unsched, 1);
    }
}
