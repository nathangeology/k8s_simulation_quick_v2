//! Conformance specs for provisioning and pod lifecycle behaviors.

use crate::conformance::{BehaviorSpec, VersionRange};
use crate::version::KarpenterVersion;

/// Returns all provisioning and lifecycle conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        ffd_bin_packing_spec(),
        deletion_cost_victim_selection_spec(),
        drain_triggers_rs_reconcile_spec(),
        spread_creates_multiple_nodes_spec(),
        antiaffinity_spreads_replicas_spec(),
        cross_antiaffinity_separates_spec(),
        nodeselector_routes_to_pool_spec(),
        nodeselector_routes_to_correct_pool_spec(),
        multi_pool_weight_preference_spec(),
        multi_pool_isolation_spec(),
    ]
}

/// Spec 1: V1 provisioner sorts batches largest-first (FFD) for bin-packing.
fn ffd_bin_packing_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
    use crate::provisioner::provision_versioned;
    use kubesim_core::*;
    use kubesim_ec2::Catalog;

    BehaviorSpec {
        name: "provisioner-ffd-bin-packing",
        description: "V1 provisioner uses first-fit-decreasing: larger batches placed first, \
                       producing a heterogeneous fleet with better packing than naive ordering",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
                labels: vec![],
                taints: vec![],
                max_disrupted_pct: 10,
                max_disrupted_count: None,
                weight: 0,
                do_not_disrupt: false,
            };
            let usage = NodePoolUsage::default();
            let mut state = ClusterState::new();

            for _ in 0..2 {
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(),
                    phase: PodPhase::Pending,
                    node: None,
                    scheduling_constraints: SchedulingConstraints::default(),
                    deletion_cost: None,
                    owner: OwnerId(1),
                    qos_class: QoSClass::Burstable,
                    priority: 0,
                    labels: LabelSet::default(),
                    do_not_disrupt: false,
                    duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }
            for _ in 0..2 {
                let mut constraints = SchedulingConstraints::default();
                constraints.node_affinity.push(NodeAffinityTerm {
                    affinity_type: AffinityType::Required,
                    match_labels: LabelSet(vec![("tier".into(), "compute".into())]),
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 3000, memory_bytes: 4 * 1024 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(),
                    phase: PodPhase::Pending,
                    node: None,
                    scheduling_constraints: constraints,
                    deletion_cost: None,
                    owner: OwnerId(2),
                    qos_class: QoSClass::Burstable,
                    priority: 0,
                    labels: LabelSet::default(),
                    do_not_disrupt: false,
                    duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }

            let decisions = provision_versioned(&state, &catalog, &pool, &usage, Some(profile), &Resources::default(), 0);

            if decisions.is_empty() {
                return Err("no provisioning decisions made".into());
            }

            let types: std::collections::HashSet<&str> =
                decisions.iter().map(|d| d.instance_type.as_str()).collect();
            if types.len() < 2 {
                return Err(format!(
                    "expected heterogeneous fleet (>=2 instance types), got {:?}",
                    types
                ));
            }
            Ok(())
        }),
    }
}

/// Spec 2: RS controller uses deletion_cost to pick scale-down victims.
fn deletion_cost_victim_selection_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::{Engine, Event, ReplicaSetController, TimeMode};

    BehaviorSpec {
        name: "rs-deletion-cost-victim-selection",
        description: "ReplicaSet scale-down deletes pods with lowest deletion_cost first, \
                       preserving high-cost pods",
        applies_to: VersionRange::all(),
        test: Box::new(|_profile| {
            let mut state = ClusterState::new();
            let node = kubesim_core::Node {
                instance_type: "m5.xlarge".into(),
                allocatable: Resources { cpu_millis: 4000, memory_bytes: 16 * 1024 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
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
            };
            let nid = state.add_node(node);

            let owner = OwnerId(1);
            let costs = [100i32, -500, 50];
            let mut pod_ids = Vec::new();
            for &cost in &costs {
                let pod = Pod {
                    requests: Resources { cpu_millis: 100, memory_bytes: 128 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(),
                    phase: PodPhase::Pending,
                    node: None,
                    scheduling_constraints: SchedulingConstraints::default(),
                    deletion_cost: Some(cost),
                    owner,
                    qos_class: QoSClass::Burstable,
                    priority: 0,
                    labels: LabelSet::default(),
                    do_not_disrupt: false,
                    duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                };
                let pid = state.submit_pod(pod);
                state.bind_pod(pid, nid);
                pod_ids.push(pid);
            }

            state.add_replica_set(ReplicaSet {
                owner_id: owner,
                desired_replicas: 1,
                pod_template: PodTemplate {
                    requests: Resources { cpu_millis: 100, memory_bytes: 128 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(),
                    priority: 0,
                    labels: LabelSet::default(),
                    scheduling_constraints: SchedulingConstraints::default(),
                    deletion_cost: None,
                },
                deletion_cost_strategy: DeletionCostStrategy::None,
            });

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            let surviving: Vec<_> = state.pods.iter()
                .filter(|(_, p)| p.owner == owner && p.phase == PodPhase::Running)
                .collect();

            if surviving.len() != 1 {
                return Err(format!("expected 1 surviving pod, got {}", surviving.len()));
            }
            let survivor_cost = surviving[0].1.deletion_cost.unwrap_or(0);
            if survivor_cost != 100 {
                return Err(format!(
                    "expected surviving pod to have cost=100 (highest), got {}",
                    survivor_cost
                ));
            }
            Ok(())
        }),
    }
}

/// Spec 3: Draining a node triggers RS reconcile so evicted pods get rescheduled.
fn drain_triggers_rs_reconcile_spec() -> BehaviorSpec {
    use crate::DrainHandler;
    use kubesim_core::*;
    use kubesim_engine::*;
    use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy};

    BehaviorSpec {
        name: "drain-triggers-rs-reconcile",
        description: "Draining a node evicts pods and triggers ReplicaSet reconcile, \
                       causing replacement pods to be created and scheduled",
        applies_to: VersionRange::all(),
        test: Box::new(|_profile| {
            let mut state = ClusterState::new();
            let mk_node = || kubesim_core::Node {
                instance_type: "m5.xlarge".into(),
                allocatable: Resources { cpu_millis: 4000, memory_bytes: 16 * 1024 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
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
            };
            let drain_nid = state.add_node(mk_node());
            let _spare_nid = state.add_node(mk_node());

            let owner = OwnerId(1);
            for _ in 0..2 {
                let pod = Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 1024 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(),
                    phase: PodPhase::Pending,
                    node: None,
                    scheduling_constraints: SchedulingConstraints::default(),
                    deletion_cost: None,
                    owner,
                    qos_class: QoSClass::Burstable,
                    priority: 0,
                    labels: LabelSet::default(),
                    do_not_disrupt: false,
                    duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                };
                let pid = state.submit_pod(pod);
                state.bind_pod(pid, drain_nid);
            }

            state.add_replica_set(ReplicaSet {
                owner_id: owner,
                desired_replicas: 2,
                pod_template: PodTemplate {
                    requests: Resources { cpu_millis: 500, memory_bytes: 1024 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(),
                    priority: 0,
                    labels: LabelSet::default(),
                    scheduling_constraints: SchedulingConstraints::default(),
                    deletion_cost: None,
                },
                deletion_cost_strategy: DeletionCostStrategy::None,
            });

            struct SchedulingGlue;
            impl EventHandler for SchedulingGlue {
                fn handle(&mut self, event: &Event, _time: SimTime, state: &mut ClusterState) -> Vec<ScheduledEvent> {
                    if matches!(event, Event::ReplicaSetReconcile(_) | Event::NodeDrained(_)) {
                        let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
                        let pending: Vec<PodId> = state.pending_queue.clone();
                        sched.schedule_pending_from(state, &pending);
                    }
                    Vec::new()
                }
                fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
            }

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(DrainHandler));
            engine.add_handler(Box::new(ReplicaSetController));
            engine.add_handler(Box::new(SchedulingGlue));

            state.nodes.get_mut(drain_nid).unwrap().cordoned = true;

            engine.schedule(SimTime(1), Event::NodeDrained(drain_nid));
            engine.run_to_completion(&mut state);

            let running: Vec<_> = state.pods.iter()
                .filter(|(_, p)| p.owner == owner && p.phase == PodPhase::Running)
                .collect();

            if running.len() != 2 {
                return Err(format!(
                    "expected 2 running pods after drain+reconcile, got {}",
                    running.len()
                ));
            }

            for (_, pod) in &running {
                if pod.node == Some(drain_nid) {
                    return Err("pod still on drained node after reconcile".into());
                }
            }
            Ok(())
        }),
    }
}

/// Spec 4: 20 pods with maxSkew=1 on hostname → multiple NodeClaims (not 1).
fn spread_creates_multiple_nodes_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
    use crate::provisioner::provision_versioned;
    use kubesim_core::*;
    use kubesim_ec2::Catalog;

    BehaviorSpec {
        name: "spread-creates-multiple-nodes",
        description: "20 pods with topologySpreadConstraints maxSkew=1 on hostname \
                       should produce ≥15 NodeClaims (not pack onto 1 node)",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(30), ..Default::default() },
                labels: vec![], taints: vec![],
                max_disrupted_pct: 10, max_disrupted_count: None, weight: 0,
                do_not_disrupt: false,
            };
            let usage = NodePoolUsage::default();
            let mut state = ClusterState::new();
            let owner = OwnerId(1);
            let labels = LabelSet(vec![("app".into(), "web".into())]);
            for _ in 0..20 {
                let mut sc = SchedulingConstraints::default();
                sc.topology_spread.push(TopologySpreadConstraint {
                    max_skew: 1,
                    topology_key: "kubernetes.io/hostname".into(),
                    when_unsatisfiable: WhenUnsatisfiable::DoNotSchedule,
                    label_selector: LabelSelector { match_labels: labels.clone() },
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner,
                    qos_class: QoSClass::Burstable, priority: 0, labels: labels.clone(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }
            let decisions = provision_versioned(&state, &catalog, &pool, &usage, Some(profile), &Resources::default(), 0);
            if decisions.len() < 15 {
                return Err(format!(
                    "expected ≥15 NodeClaims for 20 pods with maxSkew=1, got {}",
                    decisions.len()
                ));
            }
            Ok(())
        }),
    }
}

/// Spec 5: 10 pods with self anti-affinity → multiple NodeClaims.
fn antiaffinity_spreads_replicas_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
    use crate::provisioner::provision_versioned;
    use kubesim_core::*;
    use kubesim_ec2::Catalog;

    BehaviorSpec {
        name: "antiaffinity-spreads-replicas",
        description: "10 pods with required self anti-affinity on hostname \
                       should produce ≥3 NodeClaims",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(20), ..Default::default() },
                labels: vec![], taints: vec![],
                max_disrupted_pct: 10, max_disrupted_count: None, weight: 0,
                do_not_disrupt: false,
            };
            let usage = NodePoolUsage::default();
            let mut state = ClusterState::new();
            let owner = OwnerId(1);
            let labels = LabelSet(vec![("app".into(), "cache".into())]);
            for _ in 0..10 {
                let mut sc = SchedulingConstraints::default();
                sc.pod_affinity.push(PodAffinityTerm {
                    affinity_type: AffinityType::Required,
                    label_selector: LabelSelector { match_labels: labels.clone() },
                    topology_key: "kubernetes.io/hostname".into(),
                    anti: true,
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner,
                    qos_class: QoSClass::Burstable, priority: 0, labels: labels.clone(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }
            let decisions = provision_versioned(&state, &catalog, &pool, &usage, Some(profile), &Resources::default(), 0);
            if decisions.len() < 3 {
                return Err(format!(
                    "expected ≥3 NodeClaims for 10 pods with self anti-affinity, got {}",
                    decisions.len()
                ));
            }
            Ok(())
        }),
    }
}

/// Spec 6: 2 deployments with mutual anti-affinity → ≥2 NodeClaims.
fn cross_antiaffinity_separates_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
    use crate::provisioner::provision_versioned;
    use kubesim_core::*;
    use kubesim_ec2::Catalog;

    BehaviorSpec {
        name: "cross-antiaffinity-separates",
        description: "2 deployments with mutual required anti-affinity on hostname \
                       should produce ≥2 NodeClaims",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
                labels: vec![], taints: vec![],
                max_disrupted_pct: 10, max_disrupted_count: None, weight: 0,
                do_not_disrupt: false,
            };
            let usage = NodePoolUsage::default();
            let mut state = ClusterState::new();

            let labels_a = LabelSet(vec![("app".into(), "frontend".into())]);
            let labels_b = LabelSet(vec![("app".into(), "backend".into())]);

            // Deploy A: anti-affinity against B
            for _ in 0..3 {
                let mut sc = SchedulingConstraints::default();
                sc.pod_affinity.push(PodAffinityTerm {
                    affinity_type: AffinityType::Required,
                    label_selector: LabelSelector { match_labels: labels_b.clone() },
                    topology_key: "kubernetes.io/hostname".into(),
                    anti: true,
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(1),
                    qos_class: QoSClass::Burstable, priority: 0, labels: labels_a.clone(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }
            // Deploy B: anti-affinity against A
            for _ in 0..3 {
                let mut sc = SchedulingConstraints::default();
                sc.pod_affinity.push(PodAffinityTerm {
                    affinity_type: AffinityType::Required,
                    label_selector: LabelSelector { match_labels: labels_a.clone() },
                    topology_key: "kubernetes.io/hostname".into(),
                    anti: true,
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(2),
                    qos_class: QoSClass::Burstable, priority: 0, labels: labels_b.clone(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }

            let decisions = provision_versioned(&state, &catalog, &pool, &usage, Some(profile), &Resources::default(), 0);
            if decisions.len() < 2 {
                return Err(format!(
                    "expected ≥2 NodeClaims for cross anti-affinity, got {}",
                    decisions.len()
                ));
            }
            // Verify no NodeClaim has both frontend and backend pods
            for d in &decisions {
                let has_a = d.pod_ids.iter().any(|&pid| {
                    state.pods.get(pid).map_or(false, |p| p.owner == OwnerId(1))
                });
                let has_b = d.pod_ids.iter().any(|&pid| {
                    state.pods.get(pid).map_or(false, |p| p.owner == OwnerId(2))
                });
                if has_a && has_b {
                    return Err("NodeClaim contains both frontend and backend pods despite anti-affinity".into());
                }
            }
            Ok(())
        }),
    }
}

/// Spec 7: pods with nodeSelector go to matching pool only.
fn nodeselector_routes_to_pool_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits};
    use crate::provisioner::batch_pending_pods;
    use kubesim_core::*;

    BehaviorSpec {
        name: "nodeselector-routes-to-pool",
        description: "Pods with nodeSelector are only batched into matching pools",
        applies_to: VersionRange::all(),
        test: Box::new(|_profile| {
            let mut state = ClusterState::new();
            // Pod requiring spot pool
            let mut sc = SchedulingConstraints::default();
            sc.node_affinity.push(NodeAffinityTerm {
                affinity_type: AffinityType::Required,
                match_labels: LabelSet(vec![("karpenter.sh/capacity-type".into(), "spot".into())]),
            });
            state.submit_pod(Pod {
                requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                limits: Resources::default(), phase: PodPhase::Pending, node: None,
                scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(1),
                qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
            });
            // Pod requiring on-demand pool
            let mut sc2 = SchedulingConstraints::default();
            sc2.node_affinity.push(NodeAffinityTerm {
                affinity_type: AffinityType::Required,
                match_labels: LabelSet(vec![("karpenter.sh/capacity-type".into(), "on-demand".into())]),
            });
            state.submit_pod(Pod {
                requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                limits: Resources::default(), phase: PodPhase::Pending, node: None,
                scheduling_constraints: sc2, deletion_cost: None, owner: OwnerId(2),
                qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
            });

            let spot_pool = NodePool {
                name: "spot".into(),
                instance_types: vec!["m5.xlarge".into()],
                limits: NodePoolLimits::default(),
                labels: vec![("karpenter.sh/capacity-type".into(), "spot".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 0, do_not_disrupt: false,
            };
            let od_pool = NodePool {
                name: "on-demand".into(),
                instance_types: vec!["m5.xlarge".into()],
                limits: NodePoolLimits::default(),
                labels: vec![("karpenter.sh/capacity-type".into(), "on-demand".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 0, do_not_disrupt: false,
            };

            let spot_batches = batch_pending_pods(&state, Some(&spot_pool));
            let od_batches = batch_pending_pods(&state, Some(&od_pool));

            let spot_pods: usize = spot_batches.iter().map(|b| b.pod_ids.len()).sum();
            let od_pods: usize = od_batches.iter().map(|b| b.pod_ids.len()).sum();

            if spot_pods != 1 {
                return Err(format!("expected 1 pod in spot pool, got {}", spot_pods));
            }
            if od_pods != 1 {
                return Err(format!("expected 1 pod in on-demand pool, got {}", od_pods));
            }
            Ok(())
        }),
    }
}

/// Spec 8: pods with nodeSelector for capacity-type go to the correct pool and
/// are NOT batched into the wrong pool. End-to-end provisioning test.
fn nodeselector_routes_to_correct_pool_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
    use crate::provisioner::provision_versioned;
    use kubesim_core::*;
    use kubesim_ec2::Catalog;

    BehaviorSpec {
        name: "nodeselector-routes-to-correct-pool",
        description: "Pods with nodeSelector karpenter.sh/capacity-type=spot are provisioned \
                       only by the spot pool, and on-demand pods only by the on-demand pool",
        applies_to: VersionRange::all(),
        test: Box::new(|profile| {
            let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
            let mut state = ClusterState::new();

            // 5 pods requiring spot
            for _ in 0..5 {
                let mut sc = SchedulingConstraints::default();
                sc.node_affinity.push(NodeAffinityTerm {
                    affinity_type: AffinityType::Required,
                    match_labels: LabelSet(vec![("karpenter.sh/capacity-type".into(), "spot".into())]),
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(1),
                    qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }
            // 5 pods requiring on-demand
            for _ in 0..5 {
                let mut sc = SchedulingConstraints::default();
                sc.node_affinity.push(NodeAffinityTerm {
                    affinity_type: AffinityType::Required,
                    match_labels: LabelSet(vec![("karpenter.sh/capacity-type".into(), "on-demand".into())]),
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(2),
                    qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }

            let spot_pool = NodePool {
                name: "spot".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
                labels: vec![("karpenter.sh/capacity-type".into(), "spot".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 50, do_not_disrupt: false,
            };
            let od_pool = NodePool {
                name: "on-demand".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
                labels: vec![("karpenter.sh/capacity-type".into(), "on-demand".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 10, do_not_disrupt: false,
            };

            let spot_decisions = provision_versioned(&state, &catalog, &spot_pool, &NodePoolUsage::default(), Some(profile), &Resources::default(), 0);
            let od_decisions = provision_versioned(&state, &catalog, &od_pool, &NodePoolUsage::default(), Some(profile), &Resources::default(), 0);

            let spot_pods: usize = spot_decisions.iter().map(|d| d.pod_ids.len()).sum();
            let od_pods: usize = od_decisions.iter().map(|d| d.pod_ids.len()).sum();

            if spot_pods != 5 {
                return Err(format!("expected 5 pods provisioned by spot pool, got {}", spot_pods));
            }
            if od_pods != 5 {
                return Err(format!("expected 5 pods provisioned by on-demand pool, got {}", od_pods));
            }
            // Verify spot pool only got spot-requesting pods (owner 1)
            for d in &spot_decisions {
                for &pid in &d.pod_ids {
                    if let Some(p) = state.pods.get(pid) {
                        if p.owner != OwnerId(1) {
                            return Err("spot pool provisioned a non-spot pod".into());
                        }
                    }
                }
            }
            Ok(())
        }),
    }
}

/// Spec 9: unconstrained pods prefer the higher-weight pool.
fn multi_pool_weight_preference_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
    use crate::provisioner::{batch_pending_pods, provision_versioned, sort_pools_by_weight};
    use kubesim_core::*;
    use kubesim_ec2::Catalog;

    BehaviorSpec {
        name: "multi-pool-weight-preference",
        description: "Unconstrained pods (no nodeSelector) are provisioned by the \
                       higher-weight pool first when multiple pools match",
        applies_to: VersionRange::all(),
        test: Box::new(|profile| {
            let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
            let mut state = ClusterState::new();

            // 10 unconstrained pods
            for _ in 0..10 {
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: SchedulingConstraints::default(),
                    deletion_cost: None, owner: OwnerId(1),
                    qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }

            let spot_pool = NodePool {
                name: "spot".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
                labels: vec![("karpenter.sh/capacity-type".into(), "spot".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 50, do_not_disrupt: false,
            };
            let od_pool = NodePool {
                name: "on-demand".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                limits: NodePoolLimits { max_nodes: Some(10), ..Default::default() },
                labels: vec![("karpenter.sh/capacity-type".into(), "on-demand".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 10, do_not_disrupt: false,
            };

            // Sort pools by weight (higher first) — simulates Karpenter's pool ordering
            let mut pools: Vec<&NodePool> = vec![&od_pool, &spot_pool];
            sort_pools_by_weight(&mut pools);

            // The first pool should be spot (weight=50)
            if pools[0].name != "spot" {
                return Err(format!("expected spot pool first after weight sort, got {}", pools[0].name));
            }

            // Unconstrained pods match both pools; higher-weight pool provisions first
            let spot_batches = batch_pending_pods(&state, Some(&spot_pool));
            let spot_pods: usize = spot_batches.iter().map(|b| b.pod_ids.len()).sum();
            if spot_pods != 10 {
                return Err(format!(
                    "expected all 10 unconstrained pods to match spot pool, got {}",
                    spot_pods
                ));
            }

            // Provision via the preferred (spot) pool — should handle all pods
            let decisions = provision_versioned(&state, &catalog, pools[0], &NodePoolUsage::default(), Some(profile), &Resources::default(), 0);
            let provisioned: usize = decisions.iter().map(|d| d.pod_ids.len()).sum();
            if provisioned != 10 {
                return Err(format!(
                    "expected spot pool to provision all 10 pods, got {}",
                    provisioned
                ));
            }
            Ok(())
        }),
    }
}

/// Spec 10: pods cannot land on the wrong pool's nodes (isolation).
fn multi_pool_isolation_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolLimits};
    use crate::provisioner::batch_pending_pods;
    use kubesim_core::*;

    BehaviorSpec {
        name: "multi-pool-isolation",
        description: "Pods with nodeSelector for one capacity-type are never batched \
                       into a pool with a different capacity-type label",
        applies_to: VersionRange::all(),
        test: Box::new(|_profile| {
            let mut state = ClusterState::new();

            // 5 pods requiring spot
            for _ in 0..5 {
                let mut sc = SchedulingConstraints::default();
                sc.node_affinity.push(NodeAffinityTerm {
                    affinity_type: AffinityType::Required,
                    match_labels: LabelSet(vec![("karpenter.sh/capacity-type".into(), "spot".into())]),
                });
                state.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(1),
                    qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }

            let od_pool = NodePool {
                name: "on-demand".into(),
                instance_types: vec!["m5.xlarge".into()],
                limits: NodePoolLimits::default(),
                labels: vec![("karpenter.sh/capacity-type".into(), "on-demand".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 10, do_not_disrupt: false,
            };

            // Spot-requiring pods must NOT appear in on-demand pool batches
            let od_batches = batch_pending_pods(&state, Some(&od_pool));
            let od_pods: usize = od_batches.iter().map(|b| b.pod_ids.len()).sum();
            if od_pods != 0 {
                return Err(format!(
                    "expected 0 spot-requiring pods in on-demand pool, got {}",
                    od_pods
                ));
            }

            // Verify the reverse: on-demand pods don't appear in spot pool
            let mut state2 = ClusterState::new();
            for _ in 0..5 {
                let mut sc = SchedulingConstraints::default();
                sc.node_affinity.push(NodeAffinityTerm {
                    affinity_type: AffinityType::Required,
                    match_labels: LabelSet(vec![("karpenter.sh/capacity-type".into(), "on-demand".into())]),
                });
                state2.submit_pod(Pod {
                    requests: Resources { cpu_millis: 500, memory_bytes: 512 * 1024 * 1024, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources::default(), phase: PodPhase::Pending, node: None,
                    scheduling_constraints: sc, deletion_cost: None, owner: OwnerId(2),
                    qos_class: QoSClass::Burstable, priority: 0, labels: LabelSet::default(),
                    do_not_disrupt: false, duration_ns: None, is_daemonset: false, resize_policy: ResizePolicy::default(), resize_status: None,
                });
            }

            let spot_pool = NodePool {
                name: "spot".into(),
                instance_types: vec!["m5.xlarge".into()],
                limits: NodePoolLimits::default(),
                labels: vec![("karpenter.sh/capacity-type".into(), "spot".into())],
                taints: vec![], max_disrupted_pct: 10, max_disrupted_count: None,
                weight: 50, do_not_disrupt: false,
            };

            let spot_batches = batch_pending_pods(&state2, Some(&spot_pool));
            let spot_pods: usize = spot_batches.iter().map(|b| b.pod_ids.len()).sum();
            if spot_pods != 0 {
                return Err(format!(
                    "expected 0 on-demand-requiring pods in spot pool, got {}",
                    spot_pods
                ));
            }
            Ok(())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::run_conformance;
    use crate::version::{KarpenterVersion, VersionProfile};

    #[test]
    fn all_specs_pass_v1() {
        let profile = VersionProfile::new(KarpenterVersion::V1);
        let report = run_conformance(&profile, &specs());
        println!("{report}");
        assert!(report.ok(), "conformance failures:\n{report}");
    }

    #[test]
    fn all_specs_pass_v0_35() {
        let profile = VersionProfile::new(KarpenterVersion::V0_35);
        let report = run_conformance(&profile, &specs());
        println!("{report}");
        assert!(report.ok(), "conformance failures:\n{report}");
    }
}
