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
///
/// Also runs a periodic reconcile loop (every `reconcile_interval_ns`) that
/// re-checks the pending queue for unschedulable pods, catching pods that
/// were evicted by consolidation or other disruptions.
pub struct ProvisioningHandler {
    pub catalog: Catalog,
    pub pool: NodePool,
    pub usage: NodePoolUsage,
    /// Interval (ns) between provisioning loops in WallClock mode.
    pub loop_interval_ns: u64,
    /// Interval (ns) between periodic reconcile checks (default 10s).
    pub reconcile_interval_ns: u64,
    /// Version profile (reserved for future version-specific provisioning behavior).
    pub version_profile: Option<VersionProfile>,
    /// System overhead subtracted from node allocatable when checking pod fit.
    pub overhead: Resources,
    /// Percentage of raw capacity reserved for daemonsets.
    pub daemonset_pct: u32,
    /// Pods addressed by in-flight nodes (launched but not yet ready).
    inflight_pods: usize,
}

impl ProvisioningHandler {
    pub fn new(catalog: Catalog, pool: NodePool) -> Self {
        Self {
            catalog,
            pool,
            usage: NodePoolUsage::default(),
            loop_interval_ns: 5_000_000_000, // 5s default
            reconcile_interval_ns: 10_000_000_000, // 10s default
            version_profile: None,
            overhead: Resources::default(),
            daemonset_pct: 0,
            inflight_pods: 0,
        }
    }

    /// Create a handler with a specific Karpenter version profile.
    pub fn with_version(mut self, profile: VersionProfile) -> Self {
        self.version_profile = Some(profile);
        self
    }

    /// Set system overhead for provisioning decisions.
    pub fn with_overhead(mut self, overhead: Resources) -> Self {
        self.overhead = overhead;
        self
    }

    /// Set daemonset overhead percentage.
    pub fn with_daemonset_pct(mut self, pct: u32) -> Self {
        self.daemonset_pct = pct;
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
            // On NodeReady, cap inflight to current pending count.
            // Pods just got scheduled onto the newly-ready node, so inflight
            // must not exceed the remaining pending pods.
            if matches!(event, Event::NodeReady(_)) {
                self.inflight_pods = self.inflight_pods.min(state.pending_queue.len());
            }
            return Vec::new();
        };

        // Skip if all pending pods are already addressed by in-flight nodes
        if state.pending_queue.len() <= self.inflight_pods {
            // Still re-schedule reconcile in case new pods arrive
            if !state.pending_queue.is_empty() {
                return vec![ScheduledEvent {
                    time: SimTime(time.0 + self.reconcile_interval_ns),
                    event: Event::KarpenterProvisioningLoop,
                }];
            }
            return Vec::new();
        }

        let decisions = provisioner::provision_versioned(
            state, &self.catalog, &self.pool, &self.usage, self.version_profile.as_ref(), &self.overhead, self.daemonset_pct,
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

        // If progress was made and more pending pods remain, schedule an
        // immediate follow-up for faster convergence.
        let addressed_pods: usize = decisions.iter().map(|d| d.pod_ids.len()).sum();
        self.inflight_pods += addressed_pods;
        if !decisions.is_empty() && state.pending_queue.len() > addressed_pods {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + self.loop_interval_ns),
                event: Event::KarpenterProvisioningLoop,
            });
        }

        // Only schedule the next periodic reconcile if progress was made.
        // When no decisions are produced (e.g. at max_nodes), stop looping
        // to avoid burning event budget. The loop restarts on-demand when
        // new pods are submitted or evicted.
        if !decisions.is_empty() && !state.pending_queue.is_empty() {
            follow_ups.push(ScheduledEvent {
                time: SimTime(time.0 + self.reconcile_interval_ns),
                event: Event::KarpenterProvisioningLoop,
            });
        }

        follow_ups
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}