//! Cluster state machine — the central mutable state of the simulation.

use crate::arena::Arena;
use crate::types::*;

/// The central simulation state: arena-allocated nodes and pods.
pub struct ClusterState {
    pub nodes: Arena<Node>,
    pub pods: Arena<Pod>,
    pub time: SimTime,
    pub pending_queue: Vec<PodId>,
    pub pdbs: Vec<PodDisruptionBudget>,
}

impl Default for ClusterState {
    fn default() -> Self {
        Self::new()
    }
}

impl ClusterState {
    pub fn new() -> Self {
        Self {
            nodes: Arena::new(),
            pods: Arena::new(),
            time: SimTime(0),
            pending_queue: Vec::new(),
            pdbs: Vec::new(),
        }
    }

    /// Add a node, returning its handle.
    pub fn add_node(&mut self, node: Node) -> NodeId {
        self.nodes.insert(node)
    }

    /// Remove a node. Does NOT reschedule its pods — caller must handle that.
    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        self.nodes.remove(id)
    }

    /// Submit a pod. It starts in `Pending` and is added to the pending queue.
    pub fn submit_pod(&mut self, mut pod: Pod) -> PodId {
        pod.phase = PodPhase::Pending;
        pod.node = None;
        let id = self.pods.insert(pod);
        self.pending_queue.push(id);
        id
    }

    /// Bind a pending pod to a node, updating allocated resources.
    pub fn bind_pod(&mut self, pod_id: PodId, node_id: NodeId) -> bool {
        let (requests, phase) = match self.pods.get(pod_id) {
            Some(p) if p.phase == PodPhase::Pending => (p.requests, p.phase),
            _ => return false,
        };
        let _ = phase;
        let node = match self.nodes.get_mut(node_id) {
            Some(n) => n,
            None => return false,
        };
        node.allocated = node.allocated.saturating_add(&requests);
        node.pods.push(pod_id);
        let pod = self.pods.get_mut(pod_id).unwrap();
        pod.phase = PodPhase::Running;
        pod.node = Some(node_id);
        self.pending_queue.retain(|id| *id != pod_id);
        true
    }

    /// Remove a pod from its node and the cluster.
    pub fn remove_pod(&mut self, pod_id: PodId) -> Option<Pod> {
        let pod = self.pods.remove(pod_id)?;
        if let Some(node_id) = pod.node {
            if let Some(node) = self.nodes.get_mut(node_id) {
                node.allocated = node.allocated.saturating_sub(&pod.requests);
                node.pods.retain(|id| *id != pod_id);
            }
        }
        self.pending_queue.retain(|id| *id != pod_id);
        Some(pod)
    }

    /// Available (allocatable - allocated) resources on a node.
    pub fn available_resources(&self, node_id: NodeId) -> Option<Resources> {
        self.nodes.get(node_id).map(|n| n.allocatable.saturating_sub(&n.allocated))
    }

    /// Evict a running pod: unbind from node, set to Pending, re-add to pending queue.
    /// Returns true if the pod was evicted.
    pub fn evict_pod(&mut self, pod_id: PodId) -> bool {
        let (node_id, requests) = match self.pods.get(pod_id) {
            Some(p) if p.phase == PodPhase::Running => match p.node {
                Some(nid) => (nid, p.requests),
                None => return false,
            },
            _ => return false,
        };
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.allocated = node.allocated.saturating_sub(&requests);
            node.pods.retain(|id| *id != pod_id);
        }
        let pod = self.pods.get_mut(pod_id).unwrap();
        pod.phase = PodPhase::Pending;
        pod.node = None;
        self.pending_queue.push(pod_id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn submit_pod_adds_to_pending() {
        let mut state = ClusterState::new();
        let pid = state.submit_pod(test_pod(1000, 1_000_000));
        assert_eq!(state.pending_queue.len(), 1);
        assert_eq!(state.pods.get(pid).unwrap().phase, PodPhase::Pending);
    }

    #[test]
    fn bind_pod_updates_node_and_pod() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));

        assert!(state.bind_pod(pid, nid));
        let pod = state.pods.get(pid).unwrap();
        assert_eq!(pod.phase, PodPhase::Running);
        assert_eq!(pod.node, Some(nid));
        let node = state.nodes.get(nid).unwrap();
        assert_eq!(node.allocated.cpu_millis, 1000);
        assert!(state.pending_queue.is_empty());
    }

    #[test]
    fn bind_pod_fails_for_nonexistent_node() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));
        state.remove_node(nid);
        assert!(!state.bind_pod(pid, nid));
    }

    #[test]
    fn evict_pod_returns_to_pending() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));
        state.bind_pod(pid, nid);

        assert!(state.evict_pod(pid));
        let pod = state.pods.get(pid).unwrap();
        assert_eq!(pod.phase, PodPhase::Pending);
        assert_eq!(pod.node, None);
        assert_eq!(state.pending_queue.len(), 1);
        assert_eq!(state.nodes.get(nid).unwrap().allocated.cpu_millis, 0);
    }

    #[test]
    fn remove_pod_cleans_up_node() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));
        state.bind_pod(pid, nid);

        let removed = state.remove_pod(pid);
        assert!(removed.is_some());
        assert!(state.pods.get(pid).is_none());
        assert_eq!(state.nodes.get(nid).unwrap().allocated.cpu_millis, 0);
        assert!(state.nodes.get(nid).unwrap().pods.is_empty());
    }

    #[test]
    fn available_resources_computed_correctly() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));
        let pid = state.submit_pod(test_pod(1000, 2_000_000_000));
        state.bind_pod(pid, nid);

        let avail = state.available_resources(nid).unwrap();
        assert_eq!(avail.cpu_millis, 3000);
        assert_eq!(avail.memory_bytes, 6_000_000_000);
    }

    #[test]
    fn evict_pending_pod_returns_false() {
        let mut state = ClusterState::new();
        let pid = state.submit_pod(test_pod(1000, 1_000_000_000));
        assert!(!state.evict_pod(pid));
    }
}
