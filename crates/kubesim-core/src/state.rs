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
