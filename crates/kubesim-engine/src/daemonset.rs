//! DaemonSet controller — auto-creates non-evictable pods on NodeReady.

use kubesim_core::*;
use crate::{Event, EventHandler, ScheduledEvent};

/// Specification for a daemonset that runs on every node.
#[derive(Debug, Clone)]
pub struct DaemonSetSpec {
    pub name: String,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
}

/// Default logging daemonset: 150m CPU, 500Mi memory.
impl Default for DaemonSetSpec {
    fn default() -> Self {
        Self {
            name: "logging_daemonset".into(),
            cpu_millis: 150,
            memory_bytes: 500 * 1024 * 1024,
        }
    }
}

/// Handler that creates daemonset pods on every NodeReady event.
pub struct DaemonSetHandler {
    pub specs: Vec<DaemonSetSpec>,
    owner: OwnerId,
}

impl DaemonSetHandler {
    pub fn new(specs: Vec<DaemonSetSpec>) -> Self {
        Self { specs, owner: OwnerId(u32::MAX - 1) }
    }

    /// Create with the default logging daemonset.
    pub fn with_defaults() -> Self {
        Self::new(vec![DaemonSetSpec::default()])
    }
}

impl EventHandler for DaemonSetHandler {
    fn handle(
        &mut self,
        event: &Event,
        _time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        let Event::NodeReady(node_id) = event else {
            return Vec::new();
        };
        let node_id = *node_id;
        if state.nodes.get(node_id).is_none() {
            return Vec::new();
        }

        for spec in &self.specs {
            let pod = Pod {
                requests: Resources {
                    cpu_millis: spec.cpu_millis,
                    memory_bytes: spec.memory_bytes,
                    gpu: 0,
                    ephemeral_bytes: 0,
                },
                limits: Resources::default(),
                phase: PodPhase::Pending,
                node: None,
                scheduling_constraints: SchedulingConstraints::default(),
                deletion_cost: None,
                owner: self.owner,
                qos_class: QoSClass::Burstable,
                priority: 1000, // high priority — system pod
                labels: LabelSet::default(),
                do_not_disrupt: true,
                duration_ns: None,
                is_daemonset: true,
            };
            let pod_id = state.submit_pod(pod);
            state.bind_pod(pod_id, node_id);
        }

        Vec::new()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}
