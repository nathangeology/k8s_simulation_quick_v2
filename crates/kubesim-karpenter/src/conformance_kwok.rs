//! Conformance specs validating full simulation against real KWOK+Karpenter 1.9 data.
//!
//! These tests run the benchmark-control-kwok scenario end-to-end and verify that
//! provisioning behavior (node count, cost, pending time) matches real Karpenter.

use crate::conformance::{BehaviorSpec, VersionRange};
use crate::version::KarpenterVersion;

pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        cost_optimizing_provisioner_picks_large_nodes(),
        pending_pods_during_node_startup(),
        consolidation_reaches_minimal_fleet(),
        peak_cost_within_range(),
    ]
}

/// Real Karpenter picks ~11 large nodes for 1000 pods with KWOK types.
/// Sim should pick fewer than 40 nodes (not 172 like old FFD).
fn cost_optimizing_provisioner_picks_large_nodes() -> BehaviorSpec {
    BehaviorSpec {
        name: "kwok-cost-optimizing-picks-large-nodes",
        description: "Cost-per-pod provisioner selects fewer, larger instance types \
                       rather than many small ones. With 30 KWOK types and 1000 pods, \
                       should use <15 nodes (real Karpenter uses ~11).",
        applies_to: VersionRange::from(KarpenterVersion::V1),
        test: Box::new(|_profile| {
            let (peak_nodes, _, _) = run_kwok_benchmark(42)?;
            if peak_nodes > 15 {
                return Err(format!(
                    "peak nodes = {} (expected <15, real Karpenter uses ~11). \
                     Provisioner is picking too many small instance types.",
                    peak_nodes
                ));
            }
            if peak_nodes < 5 {
                return Err(format!(
                    "peak nodes = {} (expected >=5). Suspiciously low — \
                     may not be provisioning enough.",
                    peak_nodes
                ));
            }
            Ok(())
        }),
    }
}

/// With 30s node_startup delay, pods should be pending during provisioning.
/// Real Karpenter showed ~96 pending pods at t=60s. Sim should show >0 pending
/// at the first snapshot after scale-up.
fn pending_pods_during_node_startup() -> BehaviorSpec {
    BehaviorSpec {
        name: "kwok-pending-pods-during-startup",
        description: "With node startup delay, pods remain pending while nodes launch. \
                       At t=60s (first snapshot after scale-up), pending count should be >0, \
                       matching real Karpenter behavior (~96 pending).",
        applies_to: VersionRange::from(KarpenterVersion::V1),
        test: Box::new(|_profile| {
            let (_, timeseries, _) = run_kwok_benchmark(42)?;

            // Find snapshot closest to t=30s (during provisioning)
            let early_snapshot = timeseries.iter()
                .find(|s| s.elapsed_s >= 30 && s.elapsed_s <= 90);

            match early_snapshot {
                Some(s) if s.pending > 0 => Ok(()),
                Some(s) => Err(format!(
                    "at t={}s: pending={}, expected >0 during node startup. \
                     Nodes may be becoming ready too fast or inflight tracking broken.",
                    s.elapsed_s, s.pending
                )),
                None => Err("no snapshot found between t=30s and t=90s".into()),
            }
        }),
    }
}

/// After final scale-down (t=25m) + consolidation, sim should reach <=2 nodes.
/// Real Karpenter reached 1 node by t=30m.
fn consolidation_reaches_minimal_fleet() -> BehaviorSpec {
    BehaviorSpec {
        name: "kwok-consolidation-reaches-minimal-fleet",
        description: "After scale-down to 20 pods, consolidation should reduce to <=2 nodes. \
                       Real Karpenter reached 1 node by t=30m.",
        applies_to: VersionRange::from(KarpenterVersion::V1),
        test: Box::new(|_profile| {
            let (_, timeseries, _) = run_kwok_benchmark(42)?;

            // Check end state
            let end = timeseries.last()
                .ok_or("empty timeseries")?;

            if end.nodes > 2 {
                return Err(format!(
                    "end state: {} nodes (expected <=2). Consolidation may not be \
                     draining empty nodes after scale-down.",
                    end.nodes
                ));
            }
            Ok(())
        }),
    }
}

/// Peak hourly cost should be in a reasonable range.
/// Real Karpenter: $42/hr. Sim should be <$120/hr (allows for different batching).
fn peak_cost_within_range() -> BehaviorSpec {
    BehaviorSpec {
        name: "kwok-peak-cost-within-range",
        description: "Peak hourly cost with KWOK types should be <$60/hr for 1000 pods. \
                       Real Karpenter achieved $42/hr with large instance types.",
        applies_to: VersionRange::from(KarpenterVersion::V1),
        test: Box::new(|_profile| {
            let (_, timeseries, _) = run_kwok_benchmark(42)?;

            let peak_cost = timeseries.iter()
                .map(|s| s.cost_per_hour)
                .fold(0.0f64, f64::max);

            if peak_cost > 60.0 {
                return Err(format!(
                    "peak cost = ${:.2}/hr (expected <$120). Provisioner may be \
                     selecting too many expensive small nodes.",
                    peak_cost
                ));
            }
            if peak_cost < 10.0 {
                return Err(format!(
                    "peak cost = ${:.2}/hr (expected >$10). May not be provisioning enough.",
                    peak_cost
                ));
            }
            Ok(())
        }),
    }
}

// ── Helpers ─────────────────────────────────────────────────────

struct Snapshot {
    elapsed_s: u64,
    nodes: u32,
    #[allow(dead_code)]
    pods: u32,
    pending: u32,
    cost_per_hour: f64,
}

/// Run the KWOK benchmark scenario and return (peak_nodes, timeseries, end_nodes).
fn run_kwok_benchmark(seed: u64) -> Result<(u32, Vec<Snapshot>, u32), String> {
    use kubesim_core::*;
    use kubesim_ec2::Catalog;
    use kubesim_engine::*;
    use kubesim_scheduler::*;
    use crate::{
        ProvisioningHandler, ConsolidationHandler, DrainHandler,
        consolidation::ConsolidationPolicy,
    };

    let catalog = Catalog::kwok().map_err(|e| e.to_string())?;
    let catalog2 = Catalog::kwok().map_err(|e| e.to_string())?;
    let time_mode = TimeMode::WallClock;
    let mut state = ClusterState::new();
    let mut engine = Engine::new(time_mode);

    // Instance types: all amd64-linux KWOK types
    let instance_types: Vec<String> = catalog.all().iter()
        .filter(|it| it.instance_type.contains("amd64-linux"))
        .map(|it| it.instance_type.clone())
        .collect();

    let pool = crate::nodepool::NodePool {
        name: "default".into(),
        instance_types,
        limits: crate::nodepool::NodePoolLimits { max_nodes: Some(200), ..Default::default() },
        labels: vec![],
        taints: vec![],
        max_disrupted_pct: 10,
        max_disrupted_count: None,
        weight: 0,
        do_not_disrupt: false,
    };

    // Provisioner with 10s batch interval (matching Karpenter BATCH_MAX_DURATION)
    let mut prov = ProvisioningHandler::new(catalog2, pool.clone());
    prov.loop_interval_ns = 10_000_000_000; // 10s batch

    // Consolidation
    let consol = ConsolidationHandler::new(pool.clone(), ConsolidationPolicy::WhenUnderutilized);

    // Scheduler + metrics handler
    struct TestHandler {
        scheduler: Scheduler,
        catalog: Catalog,
        node_startup_ns: u64,
        snapshots: Vec<Snapshot>,
    }

    impl EventHandler for TestHandler {
        fn handle(&mut self, event: &Event, time: SimTime, state: &mut ClusterState) -> Vec<ScheduledEvent> {
            match event {
                Event::MetricsSnapshot => {
                    let kwok_nodes: u32 = state.nodes.iter()
                        .filter(|(_, n)| n.conditions.ready && !n.cordoned)
                        .count() as u32;
                    let total_pods = state.pods.iter()
                        .filter(|(_, p)| p.phase == PodPhase::Running)
                        .count() as u32;
                    let pending = state.pending_queue.len() as u32;
                    let cost: f64 = state.nodes.iter()
                        .filter(|(_, n)| n.conditions.ready)
                        .map(|(_, n)| n.cost_per_hour)
                        .sum();
                    self.snapshots.push(Snapshot {
                        elapsed_s: time.0 / 1_000_000_000,
                        nodes: kwok_nodes,
                        pods: total_pods,
                        pending,
                        cost_per_hour: cost,
                    });
                    return vec![ScheduledEvent {
                        time: SimTime(time.0 + 30_000_000_000), // every 30s
                        event: Event::MetricsSnapshot,
                    }];
                }
                Event::PodSubmitted(spec) => {
                    let pod = Pod {
                        requests: spec.requests,
                        limits: spec.limits,
                        phase: PodPhase::Pending,
                        node: None,
                        scheduling_constraints: spec.scheduling_constraints.clone(),
                        deletion_cost: None,
                        owner: spec.owner,
                        qos_class: QoSClass::Burstable,
                        priority: spec.priority,
                        labels: spec.labels.clone(),
                        do_not_disrupt: spec.do_not_disrupt,
                        duration_ns: None,
                        is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                    };
                    let pod_id = state.submit_pod(pod);
                    if let ScheduleResult::Bound(nid) = self.scheduler.schedule_one(state, pod_id) {
                        state.bind_pod(pod_id, nid);
                    } else {
                        return vec![ScheduledEvent {
                            time: SimTime(time.0 + 1),
                            event: Event::KarpenterProvisioningLoop,
                        }];
                    }
                    Vec::new()
                }
                Event::NodeLaunching(spec) => {
                    let it = self.catalog.get(&spec.instance_type);
                    let node = if let Some(it) = it {
                        Node {
                            instance_type: it.instance_type.clone(),
                            allocatable: Resources {
                                cpu_millis: (it.vcpu as u64) * 1000,
                                memory_bytes: (it.memory_gib as u64) * 1024 * 1024 * 1024,
                                gpu: 0, ephemeral_bytes: 0,
                            },
                            allocated: Resources::default(),
                            pods: smallvec::smallvec![],
                            conditions: NodeConditions { ready: false, ..Default::default() },
                            labels: spec.labels.clone(),
                            taints: smallvec::smallvec![],
                            cost_per_hour: it.on_demand_price_per_hour,
                            lifecycle: NodeLifecycle::OnDemand,
                            cordoned: false,
                            created_at: time,
                            pool_name: spec.pool_name.clone(),
                            do_not_disrupt: false,
                        }
                    } else {
                        return Vec::new();
                    };
                    let node_id = state.add_node(node);
                    vec![ScheduledEvent {
                        time: SimTime(time.0 + self.node_startup_ns),
                        event: Event::NodeReady(node_id),
                    }]
                }
                Event::NodeReady(node_id) => {
                    if let Some(n) = state.nodes.get_mut(*node_id) {
                        n.conditions.ready = true;
                    }
                    let pending: Vec<_> = state.pending_queue.clone();
                    self.scheduler.schedule_pending_from(state, &pending);
                    if !state.pending_queue.is_empty() {
                        return vec![ScheduledEvent {
                            time: SimTime(time.0 + 1),
                            event: Event::KarpenterProvisioningLoop,
                        }];
                    }
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    let handler = TestHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated)),
        catalog: Catalog::kwok().map_err(|e| e.to_string())?,
        node_startup_ns: 30_000_000_000, // 30s
        snapshots: Vec::new(),
    };

    engine.add_handler(Box::new(handler));
    engine.add_handler(Box::new(prov));
    engine.add_handler(Box::new(consol));
    engine.add_handler(Box::new(DrainHandler));
    engine.add_handler(Box::new(ReplicaSetController));

    // Create 2 ReplicaSets with 1 replica each
    let owner_a = OwnerId(1);
    let owner_b = OwnerId(2);
    let template_a = PodTemplate {
        requests: Resources { cpu_millis: 950, memory_bytes: 3_758_096_384, gpu: 0, ephemeral_bytes: 0 }, // 3.5Gi
        limits: Resources::default(),
        priority: 0,
        labels: LabelSet::default(),
        scheduling_constraints: SchedulingConstraints::default(),
    };
    let template_b = PodTemplate {
        requests: Resources { cpu_millis: 950, memory_bytes: 6_979_321_856, gpu: 0, ephemeral_bytes: 0 }, // 6.5Gi
        limits: Resources::default(),
        priority: 0,
        labels: LabelSet::default(),
        scheduling_constraints: SchedulingConstraints::default(),
    };

    state.add_replica_set(ReplicaSet {
        owner_id: owner_a, desired_replicas: 1, pod_template: template_a,
        deletion_cost_strategy: DeletionCostStrategy::None,
    });
    state.add_replica_set(ReplicaSet {
        owner_id: owner_b, desired_replicas: 1, pod_template: template_b,
        deletion_cost_strategy: DeletionCostStrategy::None,
    });

    // t=0: submit initial pods + start loops
    engine.schedule(SimTime(0), Event::ReplicaSetReconcile(owner_a));
    engine.schedule(SimTime(0), Event::ReplicaSetReconcile(owner_b));
    engine.schedule(SimTime(1), Event::KarpenterProvisioningLoop);
    engine.schedule(SimTime(1), Event::KarpenterConsolidationLoop);
    engine.schedule(SimTime(30_000_000_000), Event::MetricsSnapshot); // first snapshot at 30s

    // t=10s: scale up to 500 each
    engine.schedule(SimTime(10_000_000_000), Event::ScaleUp(DeploymentId(1), 500));
    engine.schedule(SimTime(10_000_000_000), Event::ScaleUp(DeploymentId(2), 500));

    // t=15m: scale down by 150 each
    engine.schedule(SimTime(900_000_000_000), Event::ScaleDown(DeploymentId(1), 150));
    engine.schedule(SimTime(900_000_000_000), Event::ScaleDown(DeploymentId(2), 150));

    // t=25m: scale down by 340 each
    engine.schedule(SimTime(1_500_000_000_000), Event::ScaleDown(DeploymentId(1), 340));
    engine.schedule(SimTime(1_500_000_000_000), Event::ScaleDown(DeploymentId(2), 340));

    // Run for 40 minutes
    let max_time_ns = 40 * 60 * 1_000_000_000u64;
    engine.run_until(&mut state, SimTime(max_time_ns));

    // Extract metrics
    let mut timeseries = Vec::new();
    let mut peak_nodes: u32 = 0;
    for h in engine.handlers_mut() {
        if let Some(th) = h.as_any_mut().downcast_mut::<TestHandler>() {
            timeseries = std::mem::take(&mut th.snapshots);
            break;
        }
    }
    for s in &timeseries {
        peak_nodes = peak_nodes.max(s.nodes);
    }

    let end_nodes = timeseries.last().map(|s| s.nodes).unwrap_or(0);
    Ok((peak_nodes, timeseries, end_nodes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::run_conformance;
    use crate::version::{KarpenterVersion, VersionProfile};

    #[test]
    fn all_kwok_specs_pass_v1() {
        let profile = VersionProfile::new(KarpenterVersion::V1);
        let report = run_conformance(&profile, &specs());
        println!("{report}");
        assert!(report.ok(), "KWOK conformance failures:\n{report}");
    }
}
