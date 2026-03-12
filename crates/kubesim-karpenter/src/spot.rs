//! Spot interruption modeling — stochastic interruption events for spot nodes.
//!
//! Each spot node has a per-step interruption probability (`interruption_prob`
//! stored as parts-per-million on `NodeLifecycle::Spot`). At each check interval
//! the handler rolls dice for every spot node. On interruption it emits a
//! `SpotInterruption` warning, evicts all pods back to Pending, and schedules
//! `NodeTerminated` after a 2-minute grace period.

use kubesim_core::*;
use kubesim_engine::{Event, EventHandler, ScheduledEvent};

/// Interval between spot interruption checks (default 30 s in WallClock nanos).
const DEFAULT_CHECK_INTERVAL_NS: u64 = 30_000_000_000;

/// ITN-2 grace period: 2 minutes in nanoseconds.
const SPOT_WARNING_NS: u64 = 120_000_000_000;

/// Tracks cumulative spot disruption counts.
#[derive(Debug, Default)]
pub struct SpotDisruptionMetrics {
    pub interruptions: u64,
    pub pods_disrupted: u64,
}

/// Handler that periodically checks spot nodes for interruption.
pub struct SpotInterruptionHandler {
    /// Interval between checks (ns, WallClock mode).
    pub check_interval_ns: u64,
    /// Simple xorshift RNG state.
    rng_state: u64,
    /// Cumulative metrics.
    pub metrics: SpotDisruptionMetrics,
}

impl SpotInterruptionHandler {
    pub fn new(seed: u64) -> Self {
        Self {
            check_interval_ns: DEFAULT_CHECK_INTERVAL_NS,
            rng_state: seed | 1, // ensure non-zero
            metrics: SpotDisruptionMetrics::default(),
        }
    }

    /// Xorshift64 PRNG — returns value in [0, u32::MAX].
    fn next_u32(&mut self) -> u32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        (x & 0xFFFF_FFFF) as u32
    }

    /// Returns true with probability `prob_ppm / 1_000_000`.
    fn roll(&mut self, prob_ppm: u32) -> bool {
        if prob_ppm == 0 {
            return false;
        }
        // Scale random u32 to [0, 1_000_000)
        let r = (self.next_u32() as u64 * 1_000_000 / (u32::MAX as u64 + 1)) as u32;
        r < prob_ppm
    }
}

impl EventHandler for SpotInterruptionHandler {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        match event {
            Event::SpotInterruptionCheck => {
                let mut follow_ups = Vec::new();

                // Re-schedule next check.
                follow_ups.push(ScheduledEvent {
                    time: SimTime(time.0 + self.check_interval_ns),
                    event: Event::SpotInterruptionCheck,
                });

                // Collect spot nodes to check (avoid borrow conflict with state mutation).
                let spot_nodes: Vec<(NodeId, u32)> = state
                    .nodes
                    .iter()
                    .filter_map(|(id, node)| match node.lifecycle {
                        NodeLifecycle::Spot { interruption_prob } if node.conditions.ready => {
                            Some((id, interruption_prob))
                        }
                        _ => None,
                    })
                    .collect();

                for (node_id, prob) in spot_nodes {
                    if self.roll(prob) {
                        self.metrics.interruptions += 1;

                        // Emit SpotInterruption warning (immediate).
                        follow_ups.push(ScheduledEvent {
                            time: SimTime(time.0 + 1),
                            event: Event::SpotInterruption(node_id),
                        });

                        // Schedule NodeTerminated after 2-minute grace period.
                        follow_ups.push(ScheduledEvent {
                            time: SimTime(time.0 + SPOT_WARNING_NS),
                            event: Event::NodeTerminated(node_id),
                        });
                    }
                }

                follow_ups
            }
            Event::SpotInterruption(node_id) => {
                // Evict all pods on the interrupted node back to Pending.
                let pod_ids: Vec<PodId> = state
                    .nodes
                    .get(*node_id)
                    .map(|n| n.pods.iter().copied().collect())
                    .unwrap_or_default();

                for pod_id in &pod_ids {
                    state.evict_pod(*pod_id);
                }
                self.metrics.pods_disrupted += pod_ids.len() as u64;

                // Cordon the node so no new pods get scheduled on it.
                if let Some(node) = state.nodes.get_mut(*node_id) {
                    node.conditions.ready = false;
                }

                Vec::new()
            }
            Event::NodeTerminated(node_id) => {
                // Remove the node from the cluster.
                state.remove_node(*node_id);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot_node(prob: u32) -> Node {
        Node {
            instance_type: "m5.xlarge".into(),
            allocatable: Resources { cpu_millis: 4000, memory_bytes: 8_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            allocated: Resources::default(),
            pods: smallvec::smallvec![],
            conditions: NodeConditions { ready: true, ..Default::default() },
            labels: LabelSet::default(),
            taints: smallvec::smallvec![],
            cost_per_hour: 0.076,
            lifecycle: NodeLifecycle::Spot { interruption_prob: prob },
            cordoned: false,
            created_at: SimTime(0),
            pool_name: "default".into(),
        }
    }

    fn test_pod() -> Pod {
        Pod {
            requests: Resources { cpu_millis: 500, memory_bytes: 500_000_000, gpu: 0, ephemeral_bytes: 0 },
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
    fn zero_prob_never_interrupts() {
        let mut handler = SpotInterruptionHandler::new(42);
        assert!(!handler.roll(0));
    }

    #[test]
    fn million_prob_always_interrupts() {
        let mut handler = SpotInterruptionHandler::new(42);
        assert!(handler.roll(1_000_000));
    }

    #[test]
    fn spot_interruption_evicts_pods() {
        let mut state = ClusterState::new();
        let nid = state.add_node(spot_node(0));
        let pid = state.submit_pod(test_pod());
        state.bind_pod(pid, nid);

        let mut handler = SpotInterruptionHandler::new(42);
        handler.handle(&Event::SpotInterruption(nid), SimTime(1000), &mut state);

        assert_eq!(state.pods.get(pid).unwrap().phase, PodPhase::Pending);
        assert_eq!(handler.metrics.pods_disrupted, 1);
    }

    #[test]
    fn node_terminated_removes_node() {
        let mut state = ClusterState::new();
        let nid = state.add_node(spot_node(0));

        let mut handler = SpotInterruptionHandler::new(42);
        handler.handle(&Event::NodeTerminated(nid), SimTime(1000), &mut state);

        assert!(state.nodes.get(nid).is_none());
    }

    #[test]
    fn high_prob_spot_generates_interruptions() {
        let mut state = ClusterState::new();
        // 100% interruption probability
        state.add_node(spot_node(1_000_000));

        let mut handler = SpotInterruptionHandler::new(42);
        let events = handler.handle(
            &Event::SpotInterruptionCheck,
            SimTime(1000),
            &mut state,
        );
        // Should have re-schedule + SpotInterruption + NodeTerminated
        assert!(events.len() >= 3);
        assert_eq!(handler.metrics.interruptions, 1);
    }

    #[test]
    fn on_demand_nodes_not_interrupted() {
        let mut state = ClusterState::new();
        let mut node = spot_node(0);
        node.lifecycle = NodeLifecycle::OnDemand;
        state.add_node(node);

        let mut handler = SpotInterruptionHandler::new(42);
        let events = handler.handle(
            &Event::SpotInterruptionCheck,
            SimTime(1000),
            &mut state,
        );
        // Only the self-reschedule event
        assert_eq!(events.len(), 1);
    }
}
