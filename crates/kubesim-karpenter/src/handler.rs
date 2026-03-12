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

        let decisions = provisioner::provision_versioned(
            state, &self.catalog, &self.pool, &self.usage, self.version_profile.as_ref(),
        );
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
                    labels: kubesim_core::LabelSet(self.pool.labels.clone()),
                    taints: self.pool.taints.clone(),
                    pool_name: self.pool.name.clone(),
                    do_not_disrupt: self.pool.do_not_disrupt,
                }),
            });
        }

        // Re-schedule only if we actually made progress (launched at least one node)
        // and there are still unaddressed pending pods.
        // If pending pods exist but we couldn't provision anything, re-scheduling
        // would loop forever — the same pods will fail again.
        let addressed_pods: usize = decisions.iter().map(|d| d.pod_ids.len()).sum();
        if !decisions.is_empty() && state.pending_queue.len() > addressed_pods {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + self.loop_interval_ns),
                event: Event::KarpenterProvisioningLoop,
            });
        }

        follow_ups
    }
}
