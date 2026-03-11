//! EventHandler integration — wires the provisioner into the DES engine.

use kubesim_core::*;
use kubesim_ec2::Catalog;
use kubesim_engine::{Event, EventHandler, NodeSpec, ScheduledEvent};

use crate::nodepool::{NodePool, NodePoolUsage};
use crate::provisioner;
use crate::version::VersionProfile;

/// Karpenter provisioning handler for the simulation engine.
///
/// On `KarpenterProvisioningLoop` events, batches pending pods, selects
/// instance types, and emits `NodeLaunching` events.
pub struct ProvisioningHandler {
    pub catalog: Catalog,
    pub pool: NodePool,
    pub usage: NodePoolUsage,
    /// Interval (ns) between provisioning loops in WallClock mode.
    pub loop_interval_ns: u64,
    /// Version profile (reserved for future version-specific provisioning behavior).
    pub version_profile: Option<VersionProfile>,
}

impl ProvisioningHandler {
    pub fn new(catalog: Catalog, pool: NodePool) -> Self {
        Self {
            catalog,
            pool,
            usage: NodePoolUsage::default(),
            loop_interval_ns: 5_000_000_000, // 5s default
            version_profile: None,
        }
    }

    /// Create a handler with a specific Karpenter version profile.
    pub fn with_version(mut self, profile: VersionProfile) -> Self {
        self.version_profile = Some(profile);
        self
    }
}

impl EventHandler for ProvisioningHandler {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        let Event::KarpenterProvisioningLoop = event else {
            return Vec::new();
        };

        let decisions = provisioner::provision(state, &self.catalog, &self.pool, &self.usage);
        let mut follow_ups = Vec::new();

        for decision in &decisions {
            // Update tracked usage
            if let Some(it) = self.catalog.get(&decision.instance_type) {
                self.usage.node_count += 1;
                self.usage.cpu_millis += (it.vcpu as u64) * 1000;
                self.usage.memory_bytes += (it.memory_gib as u64) * 1024 * 1024 * 1024;
            }

            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + 1),
                event: Event::NodeLaunching(NodeSpec {
                    instance_type: decision.instance_type.clone(),
                }),
            });
        }

        // Re-schedule next provisioning loop if there are still pending pods
        if !state.pending_queue.is_empty() || !decisions.is_empty() {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + self.loop_interval_ns),
                event: Event::KarpenterProvisioningLoop,
            });
        }

        follow_ups
    }
}
