//! Conformance specs for provisioning and pod lifecycle behaviors.

use crate::conformance::{BehaviorSpec, VersionRange};
use crate::version::KarpenterVersion;

/// Returns all provisioning and lifecycle conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        ffd_bin_packing_spec(),
        deletion_cost_victim_selection_spec(),
        drain_triggers_rs_reconcile_spec(),
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
                    duration_ns: None, is_daemonset: false,
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
                    duration_ns: None, is_daemonset: false,
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
                    duration_ns: None, is_daemonset: false,
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
    use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy, ScheduleResult};

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
                    duration_ns: None, is_daemonset: false,
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
                },
                deletion_cost_strategy: DeletionCostStrategy::None,
            });

            struct SchedulingGlue;
            impl EventHandler for SchedulingGlue {
                fn handle(&mut self, event: &Event, _time: SimTime, state: &mut ClusterState) -> Vec<ScheduledEvent> {
                    if matches!(event, Event::ReplicaSetReconcile(_) | Event::NodeDrained(_)) {
                        let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
                        let pending: Vec<PodId> = state.pending_queue.clone();
                        for pid in pending {
                            if let ScheduleResult::Bound(nid) = sched.schedule_one(state, pid) {
                                state.bind_pod(pid, nid);
                            }
                        }
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
