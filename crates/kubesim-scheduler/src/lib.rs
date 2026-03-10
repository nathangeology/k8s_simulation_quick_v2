//! KubeSim Scheduler — Filter and Score plugin chain modelling kube-scheduler.
//!
//! Implements the two-phase scheduling pipeline:
//! 1. **Filter**: eliminate nodes that cannot run the pod
//! 2. **Score**: rank remaining nodes, pick the highest
//!
//! Built-in plugins:
//! - Filters: `NodeResourcesFit`, `TaintToleration`, `NodeAffinity`
//! - Scorers: `MostAllocated`, `LeastAllocated`, `NodeAffinityScore`

pub use kubesim_core;

use kubesim_core::{AffinityType, ClusterState, Node, NodeId, Pod, PodId, Resources, Taint};

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
            filters: vec![Box::new(NodeResourcesFit), Box::new(TaintToleration), Box::new(NodeAffinity)],
            scorers: vec![scorer, Box::new(NodeAffinityScore)],
        }
    }
}

/// High-level scoring strategy selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoringStrategy {
    MostAllocated,
    LeastAllocated,
}

// ── Scheduler ───────────────────────────────────────────────────

/// The result of scheduling a single pod.
#[derive(Debug)]
pub enum ScheduleResult {
    /// Pod was bound to this node.
    Bound(NodeId),
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
            if !node.conditions.ready {
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
            return ScheduleResult::Unschedulable(reasons);
        }

        // Score phase
        let best = feasible
            .iter()
            .map(|&(nid, node)| {
                let total: i64 = self
                    .profile
                    .scorers
                    .iter()
                    .map(|s| s.score(state, pod, node) * s.weight())
                    .sum();
                (nid, total)
            })
            .max_by_key(|&(_, score)| score)
            .map(|(nid, _)| nid)
            .unwrap(); // feasible is non-empty

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
