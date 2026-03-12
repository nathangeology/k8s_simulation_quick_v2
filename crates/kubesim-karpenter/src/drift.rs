//! Drift detection — identifies nodes that no longer match their NodePool spec
//! and orchestrates cordon → drain → terminate replacement.

use kubesim_core::*;
use kubesim_engine::{Event, EventHandler, ScheduledEvent};

use crate::nodepool::NodePool;
use crate::version::VersionProfile;

/// Configuration for drift detection behavior.
#[derive(Debug, Clone)]
pub struct DriftConfig {
    /// Interval (ns) between drift detection scans in WallClock mode.
    pub scan_interval_ns: u64,
    /// Graceful drain timeout (ns). After this, remaining pods are force-evicted.
    pub drain_timeout_ns: u64,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            scan_interval_ns: 30_000_000_000,  // 30s
            drain_timeout_ns: 300_000_000_000, // 5min
        }
    }
}

/// Tracks a node undergoing drift replacement (cordon → drain → terminate).
#[derive(Debug, Clone)]
struct DriftingNode {
    node_id: NodeId,
    drain_deadline: SimTime,
}

/// Drift detection handler for the simulation engine.
///
/// On `KarpenterConsolidationLoop` events (reused for drift scans), checks all
/// nodes against the current NodePool spec. Drifted nodes are cordoned, drained
/// respecting PDBs, and terminated after drain completes or timeout expires.
pub struct DriftHandler {
    pub pool: NodePool,
    pub config: DriftConfig,
    /// Nodes currently being drained due to drift.
    draining: Vec<DriftingNode>,
    /// Version profile controlling drift detection behavior.
    pub version_profile: Option<VersionProfile>,
}

impl DriftHandler {
    pub fn new(pool: NodePool, config: DriftConfig) -> Self {
        Self { pool, config, draining: Vec::new(), version_profile: None }
    }

    /// Create a handler with a specific Karpenter version profile.
    pub fn with_version(mut self, profile: VersionProfile) -> Self {
        self.version_profile = Some(profile);
        self
    }

    /// Check if a node is drifted. v0.35: instance type only. v1.x: also hash-based.
    pub fn is_drifted(&self, node: &Node) -> bool {
        if self.pool.instance_types.is_empty() {
            return false;
        }
        let type_drifted = !self.pool.instance_types.iter().any(|t| t == &node.instance_type);

        // v0.35: only instance type (AMI) drift
        // v1.x: also detects label/taint drift via hash comparison
        let hash_drift = self.version_profile
            .as_ref()
            .map_or(true, |p| p.hash_based_drift)
            && self.has_label_drift(node);

        type_drifted || hash_drift
    }

    /// Check if node labels diverge from the NodePool's expected labels.
    fn has_label_drift(&self, node: &Node) -> bool {
        self.pool.labels.iter().any(|(k, v)| node.labels.get(k) != Some(v.as_str()))
    }

    /// Count how many pods matching a PDB's selector are currently Running.
    fn count_available(state: &ClusterState, pdb: &PodDisruptionBudget) -> u32 {
        state.pods.iter()
            .filter(|(_, p)| p.phase == PodPhase::Running && p.labels.matches(&pdb.selector))
            .count() as u32
    }

    /// Check if evicting a pod would violate any PDB.
    fn pdb_allows_eviction(state: &ClusterState, pod: &Pod) -> bool {
        for pdb in &state.pdbs {
            if pod.labels.matches(&pdb.selector) {
                let available = Self::count_available(state, pdb);
                if available <= pdb.min_available {
                    return false;
                }
            }
        }
        true
    }

    /// Try to drain pods from a node, respecting PDBs. Returns true if node is empty.
    fn try_drain(state: &mut ClusterState, node_id: NodeId) -> bool {
        let pod_ids: Vec<PodId> = match state.nodes.get(node_id) {
            Some(n) => n.pods.clone().into_vec(),
            None => return true,
        };

        if pod_ids.is_empty() {
            return true;
        }

        let mut evictable = Vec::new();
        for &pid in &pod_ids {
            if let Some(pod) = state.pods.get(pid) {
                if pod.phase == PodPhase::Running && Self::pdb_allows_eviction(state, pod) {
                    evictable.push(pid);
                }
            }
        }

        for pid in evictable {
            state.evict_pod(pid);
        }

        // Check if node is now empty
        state.nodes.get(node_id).map_or(true, |n| n.pods.is_empty())
    }

    /// Force-evict all remaining pods (drain timeout expired).
    fn force_drain(state: &mut ClusterState, node_id: NodeId) {
        let pod_ids: Vec<PodId> = match state.nodes.get(node_id) {
            Some(n) => n.pods.clone().into_vec(),
            None => return,
        };
        for pid in pod_ids {
            state.evict_pod(pid);
        }
    }
}

impl EventHandler for DriftHandler {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        let Event::KarpenterConsolidationLoop = event else {
            return Vec::new();
        };

        let mut follow_ups = Vec::new();

        // Phase 1: Process nodes already draining — try to finish drain or force on timeout
        let mut still_draining = Vec::new();
        for entry in self.draining.drain(..) {
            if state.nodes.get(entry.node_id).is_none() {
                continue; // already removed
            }
            if time >= entry.drain_deadline {
                // Timeout: force-evict remaining pods and terminate
                Self::force_drain(state, entry.node_id);
                follow_ups.push(ScheduledEvent {
                    time: SimTime(time.0 + 1),
                    event: Event::NodeTerminated(entry.node_id),
                });
                state.remove_node(entry.node_id);
            } else if Self::try_drain(state, entry.node_id) {
                // Drain complete — terminate
                follow_ups.push(ScheduledEvent {
                    time: SimTime(time.0 + 1),
                    event: Event::NodeTerminated(entry.node_id),
                });
                state.remove_node(entry.node_id);
            } else {
                still_draining.push(entry);
            }
        }
        self.draining = still_draining;

        // Phase 2: Scan for newly drifted nodes
        let drifted: Vec<NodeId> = state.nodes.iter()
            .filter(|(id, node)| {
                self.is_drifted(node)
                    && node.conditions.ready
                    && !self.draining.iter().any(|d| d.node_id == *id)
            })
            .map(|(id, _)| id)
            .collect();

        for node_id in drifted {
            // Cordon the node
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + 1),
                event: Event::NodeCordoned(node_id),
            });

            let deadline = SimTime(time.0 + self.config.drain_timeout_ns);

            // Attempt immediate drain
            if Self::try_drain(state, node_id) {
                // Already empty — terminate immediately
                follow_ups.push(ScheduledEvent {
                    time: SimTime(time.0 + 2),
                    event: Event::NodeTerminated(node_id),
                });
                state.remove_node(node_id);
            } else {
                self.draining.push(DriftingNode { node_id, drain_deadline: deadline });
            }
        }

        // Trigger provisioning loop so provisioner can replace terminated nodes
        if !follow_ups.is_empty() {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + 3),
                event: Event::KarpenterProvisioningLoop,
            });
        }

        // Re-schedule next drift scan if there are draining nodes or we should keep scanning
        follow_ups.push(ScheduledEvent {
            time: SimTime(time.0 + self.config.scan_interval_ns),
            event: Event::KarpenterConsolidationLoop,
        });

        follow_ups
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubesim_engine::Event;

    fn test_node(instance_type: &str) -> Node {
        Node {
            instance_type: instance_type.into(),
            allocatable: Resources { cpu_millis: 4000, memory_bytes: 8_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            allocated: Resources::default(),
            pods: smallvec::smallvec![],
            conditions: NodeConditions { ready: true, ..Default::default() },
            labels: LabelSet::default(),
            taints: smallvec::smallvec![],
            cost_per_hour: 0.192,
            lifecycle: NodeLifecycle::OnDemand,
            cordoned: false,
            created_at: SimTime(0),
            pool_name: String::new(),
            do_not_disrupt: false,
        }
    }

    fn test_pool() -> NodePool {
        NodePool {
            name: "default".into(),
            instance_types: vec!["m5.xlarge".into()],
            limits: crate::nodepool::NodePoolLimits::default(),
            labels: vec![],
            taints: vec![],
            max_disrupted_pct: 10,
            max_disrupted_count: None,
            weight: 0,
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
        }
    }

    #[test]
    fn drifted_node_detected() {
        let pool = test_pool(); // allows only m5.xlarge
        let handler = DriftHandler::new(pool, DriftConfig::default());
        let node = test_node("m5.2xlarge"); // not in pool
        assert!(handler.is_drifted(&node));
    }

    #[test]
    fn non_drifted_node_passes() {
        let pool = test_pool();
        let handler = DriftHandler::new(pool, DriftConfig::default());
        let node = test_node("m5.xlarge");
        assert!(!handler.is_drifted(&node));
    }

    #[test]
    fn empty_pool_means_no_drift() {
        let pool = NodePool {
            instance_types: vec![],
            ..test_pool()
        };
        let handler = DriftHandler::new(pool, DriftConfig::default());
        let node = test_node("anything");
        assert!(!handler.is_drifted(&node));
    }

    #[test]
    fn drift_handler_terminates_empty_drifted_node() {
        let mut state = ClusterState::new();
        state.add_node(test_node("m5.2xlarge")); // drifted

        let mut handler = DriftHandler::new(test_pool(), DriftConfig::default());
        let events = handler.handle(
            &Event::KarpenterConsolidationLoop,
            SimTime(1000),
            &mut state,
        );

        let has_terminated = events.iter().any(|e| matches!(e.event, Event::NodeTerminated(_)));
        assert!(has_terminated);
    }

    #[test]
    fn drift_handler_drains_node_with_pods() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node("m5.2xlarge")); // drifted
        let pid = state.submit_pod(test_pod(500, 500_000_000));
        state.bind_pod(pid, nid);

        // Add a target node for rescheduling
        state.add_node(test_node("m5.xlarge"));

        let mut handler = DriftHandler::new(test_pool(), DriftConfig::default());
        handler.handle(
            &Event::KarpenterConsolidationLoop,
            SimTime(1000),
            &mut state,
        );

        // Pod should be evicted back to pending
        assert_eq!(state.pods.get(pid).unwrap().phase, PodPhase::Pending);
    }

    #[test]
    fn drift_handler_respects_pdb() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node("m5.2xlarge")); // drifted
        let mut pod = test_pod(500, 500_000_000);
        pod.labels.insert("app".into(), "web".into());
        let pid = state.submit_pod(pod);
        state.bind_pod(pid, nid);

        // PDB requires 1 pod available
        state.pdbs.push(PodDisruptionBudget {
            selector: LabelSelector { match_labels: LabelSet(vec![("app".into(), "web".into())]) },
            min_available: 1,
        });

        let mut handler = DriftHandler::new(test_pool(), DriftConfig::default());
        handler.handle(
            &Event::KarpenterConsolidationLoop,
            SimTime(1000),
            &mut state,
        );

        // Pod should NOT be evicted (PDB blocks it), node enters draining
        assert_eq!(state.pods.get(pid).unwrap().phase, PodPhase::Running);
        assert!(!handler.draining.is_empty());
    }

    #[test]
    fn drift_handler_ignores_non_consolidation_events() {
        let mut state = ClusterState::new();
        let mut handler = DriftHandler::new(test_pool(), DriftConfig::default());
        let events = handler.handle(&Event::MetricsSnapshot, SimTime(1000), &mut state);
        // Only returns empty for non-consolidation events
        assert!(events.is_empty());
    }
}
