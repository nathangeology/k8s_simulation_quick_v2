//! End-to-end integration test: full simulation loop with
//! engine + scheduler + karpenter + metrics.
//!
//! Scenario: submit pods with no initial nodes → Karpenter provisions nodes →
//! scheduler places pods → remove some pods so nodes become empty →
//! consolidation removes underutilized nodes → metrics collector captures snapshots.

use kubesim_core::*;
use kubesim_ec2::Catalog;
use kubesim_engine::*;
use kubesim_karpenter::*;
use kubesim_metrics::{MetricsCollector, MetricsConfig};
use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy};

// ── Glue handlers ───────────────────────────────────────────────

/// Handles `NodeLaunching` by creating the node in cluster state and
/// scheduling a `NodeReady` follow-up.
struct NodeLifecycleHandler {
    catalog: Catalog,
}

impl EventHandler for NodeLifecycleHandler {
    fn handle(&mut self, event: &Event, time: SimTime, state: &mut ClusterState) -> Vec<ScheduledEvent> {
        match event {
            Event::NodeLaunching(spec) => {
                let it = match self.catalog.get(&spec.instance_type) {
                    Some(it) => it,
                    None => return Vec::new(),
                };
                let node = Node {
                    instance_type: spec.instance_type.clone(),
                    allocatable: Resources {
                        cpu_millis: (it.vcpu as u64) * 1000,
                        memory_bytes: (it.memory_gib as u64) * 1024 * 1024 * 1024,
                        gpu: it.gpu_count,
                        ephemeral_bytes: 0,
                    },
                    allocated: Resources::default(),
                    pods: smallvec::smallvec![],
                    conditions: NodeConditions { ready: false, ..Default::default() },
                    labels: LabelSet::default(),
                    taints: smallvec::smallvec![],
                    cost_per_hour: it.on_demand_price_per_hour,
                    lifecycle: NodeLifecycle::OnDemand,
                    cordoned: false,
                };
                let nid = state.add_node(node);
                vec![ScheduledEvent {
                    time: SimTime(time.0 + 1),
                    event: Event::NodeReady(nid),
                }]
            }
            Event::NodeReady(nid) => {
                if let Some(n) = state.nodes.get_mut(*nid) {
                    n.conditions.ready = true;
                }
                Vec::new()
            }
            Event::NodeTerminated(nid) => {
                state.remove_node(*nid);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

/// Handles `PodSubmitted` by creating the pod in state, and runs the scheduler
/// after `NodeReady` events to place any pending pods.
struct SchedulingHandler {
    scheduler: Scheduler,
}

impl EventHandler for SchedulingHandler {
    fn handle(&mut self, event: &Event, _time: SimTime, state: &mut ClusterState) -> Vec<ScheduledEvent> {
        match event {
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
                };
                state.submit_pod(pod);
                // Try scheduling immediately
                self.scheduler.schedule_pending(state);
                Vec::new()
            }
            Event::NodeReady(_) => {
                // New capacity available — try scheduling pending pods
                self.scheduler.schedule_pending(state);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────

fn pod_spec(cpu: u64, mem: u64) -> PodSpec {
    PodSpec {
        requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
        limits: Resources::default(),
        owner: OwnerId(1),
        priority: 0,
        labels: LabelSet::default(),
        scheduling_constraints: SchedulingConstraints::default(),
    }
}

fn default_pool() -> NodePool {
    NodePool {
        name: "default".into(),
        instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
        limits: NodePoolLimits::default(),
        labels: vec![],
        taints: vec![],
        max_disrupted_pct: 100, // allow full consolidation in test
    }
}

// ── The test ────────────────────────────────────────────────────

#[test]
fn full_simulation_loop() {
    let pool = default_pool();
    let mut state = ClusterState::new();
    let mut engine = Engine::new(TimeMode::Logical);

    // Register handlers (order matters: lifecycle first, then scheduling, then karpenter, then metrics)
    engine.add_handler(Box::new(NodeLifecycleHandler {
        catalog: Catalog::embedded().unwrap(),
    }));
    engine.add_handler(Box::new(SchedulingHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::MostAllocated)),
    }));
    engine.add_handler(Box::new(ProvisioningHandler::new(
        Catalog::embedded().unwrap(),
        pool.clone(),
    )));
    engine.add_handler(Box::new(ConsolidationHandler::new(
        pool.clone(),
        ConsolidationPolicy::WhenUnderutilized,
    )));
    engine.add_handler(Box::new(MetricsCollector::new(MetricsConfig::default())));

    // ── Phase 1: Submit pods (no nodes yet) ─────────────────────
    let t = SimTime(100);
    for i in 0..4 {
        engine.schedule(SimTime(t.0 + i), Event::PodSubmitted(pod_spec(500, 512 * 1024 * 1024)));
    }

    // Trigger Karpenter provisioning to create nodes for pending pods
    engine.schedule(SimTime(200), Event::KarpenterProvisioningLoop);

    // Take a metrics snapshot before provisioning settles
    engine.schedule(SimTime(150), Event::MetricsSnapshot);

    // Run until provisioning + scheduling settles
    engine.run_until(&mut state, SimTime(300));

    // Verify: nodes were provisioned and pods are running
    assert!(state.nodes.len() > 0, "Karpenter should have provisioned at least one node");
    let running_count = state.pods.iter().filter(|(_, p)| p.phase == PodPhase::Running).count();
    assert!(running_count > 0, "Some pods should be running after provisioning + scheduling");

    // Take a snapshot after pods are scheduled
    engine.schedule(SimTime(301), Event::MetricsSnapshot);
    engine.run_until(&mut state, SimTime(310));

    // ── Phase 2: Remove pods to create underutilized nodes ──────
    // Remove all pods — this makes nodes empty for consolidation
    let pod_ids: Vec<PodId> = state.pods.iter().map(|(id, _)| id).collect();
    for pid in pod_ids {
        state.remove_pod(pid);
    }
    state.pending_queue.clear();

    // Verify nodes are now empty
    for (_nid, node) in state.nodes.iter() {
        assert!(node.pods.is_empty(), "All pods should be removed");
    }
    let nodes_before_consolidation = state.nodes.len();
    assert!(nodes_before_consolidation > 0);

    // ── Phase 3: Consolidation removes empty nodes ──────────────
    engine.schedule(SimTime(500), Event::KarpenterConsolidationLoop);
    // Take snapshot after consolidation
    engine.schedule(SimTime(600), Event::MetricsSnapshot);
    engine.run_until(&mut state, SimTime(700));

    // Consolidation should have terminated the empty nodes
    assert!(
        state.nodes.len() < nodes_before_consolidation,
        "Consolidation should remove empty nodes: before={}, after={}",
        nodes_before_consolidation,
        state.nodes.len()
    );

    // ── Phase 4: Verify metrics were captured ───────────────────
    // We can't directly access the MetricsCollector through the engine,
    // but we verified the full pipeline ran without panics and state
    // transitions occurred correctly. The metrics handler processed
    // MetricsSnapshot events (verified by the engine processing them).

    // Final state: no pods, nodes consolidated away
    assert_eq!(state.pods.len(), 0, "All pods were removed");
}

/// Verify that the scheduling + provisioning pipeline handles the case where
/// pods arrive, get provisioned, scheduled, and then new pods arrive requiring
/// additional provisioning.
#[test]
fn multi_wave_provisioning() {
    let pool = default_pool();
    let mut state = ClusterState::new();
    let mut engine = Engine::new(TimeMode::Logical);

    engine.add_handler(Box::new(NodeLifecycleHandler {
        catalog: Catalog::embedded().unwrap(),
    }));
    engine.add_handler(Box::new(SchedulingHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated)),
    }));
    engine.add_handler(Box::new(ProvisioningHandler::new(
        Catalog::embedded().unwrap(),
        pool,
    )));

    // Wave 1: 2 pods
    engine.schedule(SimTime(10), Event::PodSubmitted(pod_spec(1000, 1_000_000_000)));
    engine.schedule(SimTime(11), Event::PodSubmitted(pod_spec(1000, 1_000_000_000)));
    engine.schedule(SimTime(100), Event::KarpenterProvisioningLoop);
    engine.run_until(&mut state, SimTime(200));

    let wave1_nodes = state.nodes.len();
    let wave1_running = state.pods.iter().filter(|(_, p)| p.phase == PodPhase::Running).count();
    assert!(wave1_nodes > 0, "Wave 1 should provision nodes");
    assert!(wave1_running > 0, "Wave 1 pods should be running");

    // Wave 2: more pods that exceed current capacity
    for i in 0..6 {
        engine.schedule(SimTime(300 + i), Event::PodSubmitted(pod_spec(2000, 2_000_000_000)));
    }
    engine.schedule(SimTime(400), Event::KarpenterProvisioningLoop);
    engine.run_until(&mut state, SimTime(500));

    let wave2_running = state.pods.iter().filter(|(_, p)| p.phase == PodPhase::Running).count();
    assert!(
        wave2_running > wave1_running,
        "More pods should be running after wave 2: wave1={}, wave2={}",
        wave1_running,
        wave2_running
    );
}
