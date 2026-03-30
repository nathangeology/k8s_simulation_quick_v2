//! End-to-end integration test: full simulation loop with
//! engine + scheduler + karpenter (provisioning & consolidation) + metrics.
//!
//! Scenario:
//!   1. Submit pods that exceed initial node capacity → pods stay pending
//!   2. Karpenter provisioning loop fires → new nodes launched
//!   3. Scheduler places pods on the new nodes
//!   4. Some pods are removed → nodes become underutilized
//!   5. Consolidation loop fires → underutilized nodes drained & terminated
//!   6. Metrics collector captures snapshots throughout

use kubesim_core::*;
use kubesim_ec2::Catalog;
use kubesim_engine::*;
use kubesim_karpenter::{
    ConsolidationHandler, ConsolidationPolicy, DrainHandler, NodePool, NodePoolLimits, ProvisioningHandler,
};
use kubesim_metrics::{MetricsCollector, MetricsConfig};
use kubesim_scheduler::{ScheduleResult, Scheduler, SchedulerProfile, ScoringStrategy};

// ── Glue handler: materializes nodes, removes terminated nodes, schedules pods ──

struct SimGlueHandler {
    scheduler: Scheduler,
    catalog: Catalog,
}

impl SimGlueHandler {
    fn new(catalog: Catalog, scoring: ScoringStrategy) -> Self {
        Self {
            scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", scoring)),
            catalog,
        }
    }
}

impl EventHandler for SimGlueHandler {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
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
                    do_not_disrupt: spec.do_not_disrupt,
                    duration_ns: spec.duration_ns,
                    is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                };
                let pod_id = state.submit_pod(pod);
                if let ScheduleResult::Bound(node_id) =
                    self.scheduler.schedule_one(state, pod_id)
                {
                    state.bind_pod(pod_id, node_id);
                }
                Vec::new()
            }
            Event::NodeTerminated(node_id) => {
                state.remove_node(*node_id);
                Vec::new()
            }
            Event::KarpenterProvisioningLoop => {
                // Try to schedule pending pods onto existing nodes before the
                // provisioner launches new ones. This catches evicted pods that
                // were returned to pending by DrainHandler.
                let pending: Vec<PodId> = state.pending_queue.clone();
                self.scheduler.schedule_pending_from(state, &pending);
                Vec::new()
            }
            Event::NodeLaunching(spec) => {
                if let Some(it) = self.catalog.get(&spec.instance_type) {
                    let node = Node {
                        instance_type: it.instance_type.clone(),
                        allocatable: Resources {
                            cpu_millis: (it.vcpu as u64) * 1000,
                            memory_bytes: (it.memory_gib as u64) * 1024 * 1024 * 1024,
                            gpu: it.gpu_count,
                            ephemeral_bytes: 0,
                        },
                        allocated: Resources::default(),
                        pods: smallvec::smallvec![],
                        conditions: NodeConditions { ready: true, ..Default::default() },
                        labels: spec.labels.clone(),
                        taints: spec.taints.iter().cloned().collect(),
                        cost_per_hour: it.on_demand_price_per_hour,
                        lifecycle: NodeLifecycle::OnDemand,
                        cordoned: false,
                        created_at: time,
                        pool_name: spec.pool_name.clone(),
                        do_not_disrupt: false,
                    };
                    state.add_node(node);
                    // Schedule pending pods onto available nodes
                    let pending: Vec<PodId> = state.pending_queue.clone();
                    self.scheduler.schedule_pending_from(state, &pending);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

// ── The test ────────────────────────────────────────────────────

#[test]
fn full_simulation_loop() {
    let pool = NodePool {
        name: "default".into(),
        instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
        limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
        labels: vec![],
        taints: vec![],
        max_disrupted_pct: 50,
        max_disrupted_count: None,
        weight: 0,
        do_not_disrupt: false,
    };

    let mut state = ClusterState::new();
    let mut engine = Engine::new(TimeMode::Logical);

    // Register handlers (order matters: glue first so nodes/pods exist for others)
    engine.add_handler(Box::new(SimGlueHandler::new(
        Catalog::embedded().unwrap(),
        ScoringStrategy::LeastAllocated,
    )));
    engine.add_handler(Box::new(ProvisioningHandler::new(
        Catalog::embedded().unwrap(),
        pool.clone(),
    ).with_logical_mode()));
    engine.add_handler(Box::new(ConsolidationHandler::new(
        pool.clone(),
        ConsolidationPolicy::WhenUnderutilized,
    ).with_logical_mode()));
    engine.add_handler(Box::new(DrainHandler));
    engine.add_handler(Box::new(MetricsCollector::new(MetricsConfig::default())));

    // ── Phase 1: Submit pods that need more capacity ────────────
    // m5.xlarge = 4 vCPU (4000m), 16 GiB.
    // The provisioner batches pods by (required_labels, tolerations, gpu).
    // Give each pair of pods different tolerations → 3 batches of 2 pods each.
    // Each batch: 2×1500m = 3000m CPU, 2×3GiB = 6GiB → fits on m5.xlarge.
    for i in 0..6u64 {
        let group = i / 2;
        let mut constraints = SchedulingConstraints::default();
        constraints.tolerations.push(Toleration {
            key: format!("group-{group}"),
            operator: TolerationOperator::Exists,
            value: String::new(),
            effect: None,
        });
        engine.schedule(
            SimTime(10 + i),
            Event::PodSubmitted(PodSpec {
                requests: Resources {
                    cpu_millis: 1500,
                    memory_bytes: 3 * 1024 * 1024 * 1024,
                    gpu: 0,
                    ephemeral_bytes: 0,
                },
                limits: Resources::default(),
                owner: OwnerId(1),
                priority: 0,
                labels: LabelSet::default(),
                scheduling_constraints: constraints,
                do_not_disrupt: false,
                duration_ns: None,
            }),
        );
    }

    // ── Phase 2: Trigger Karpenter provisioning ─────────────────
    engine.schedule(SimTime(100), Event::KarpenterProvisioningLoop);
    engine.schedule(SimTime(200), Event::MetricsSnapshot);

    // Process provisioning phase
    engine.run_until(&mut state, SimTime(300));

    // Verify: nodes were provisioned and pods are running
    assert!(
        state.nodes.len() >= 2,
        "Karpenter should have provisioned nodes, got {}",
        state.nodes.len()
    );
    let running_count = state
        .pods
        .iter()
        .filter(|(_, p)| p.phase == PodPhase::Running)
        .count();
    assert_eq!(running_count, 6, "all 6 pods should be running");
    assert!(state.pending_queue.is_empty(), "no pods should be pending");

    // ── Phase 3: Remove pods to create underutilization ─────────
    let pods_to_remove: Vec<PodId> = state
        .pods
        .iter()
        .filter(|(_, p)| p.phase == PodPhase::Running)
        .map(|(id, _)| id)
        .take(4)
        .collect();
    for pid in &pods_to_remove {
        state.remove_pod(*pid);
    }

    let nodes_before = state.nodes.len();

    // ── Phase 4: Trigger consolidation ──────────────────────────
    // In logical mode, consolidate_after is 15 ticks (seconds).
    // Nodes were created at ~t=101, so they're eligible at t=116+.
    let consol_time = 200;
    state.time = SimTime(consol_time);
    engine.schedule(SimTime(consol_time), Event::KarpenterConsolidationLoop);
    engine.schedule(SimTime(consol_time + 100), Event::MetricsSnapshot);
    engine.run_until(&mut state, SimTime(consol_time + 200));

    // Verify: consolidation removed underutilized/empty nodes
    assert!(
        state.nodes.len() < nodes_before,
        "consolidation should remove nodes: before={nodes_before}, after={}",
        state.nodes.len()
    );

    // ── Phase 5: Final verification ─────────────────────────────
    engine.schedule(SimTime(consol_time + 300), Event::MetricsSnapshot);
    engine.run_until(&mut state, SimTime(consol_time + 400));

    // Remaining pods should still be alive
    let final_alive = state
        .pods
        .iter()
        .filter(|(_, p)| p.phase == PodPhase::Running || p.phase == PodPhase::Pending)
        .count();
    assert!(final_alive > 0, "surviving pods should still exist");
}
