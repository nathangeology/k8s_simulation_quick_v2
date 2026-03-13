//! Conformance specs for ReplicaSet scale-down victim selection order.

use crate::conformance::{BehaviorSpec, VersionRange};

/// Returns all ReplicaSet scale-down conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        pending_pods_first_spec(),
        deletion_cost_ordering_spec(),
        node_replica_spread_spec(),
        creation_time_recency_spec(),
        combined_ordering_spec(),
        deletion_cost_overrides_node_spread_spec(),
    ]
}

fn pending_pods_first_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::*;

    BehaviorSpec {
        name: "rs-scaledown-pending-first",
        description: "Pending (unschedulable) pods are scaled down before running pods",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let owner = OwnerId(1);
            let node_id = state.add_node(mk_node());

            let rs = state.add_replica_set(mk_rs(owner, 4));

            for _ in 0..2 {
                let pid = state.submit_pod(mk_pod(owner));
                state.bind_pod(pid, node_id);
            }
            let pending1 = state.submit_pod(mk_pod(owner));
            let pending2 = state.submit_pod(mk_pod(owner));

            state.replica_sets.get_mut(rs).unwrap().desired_replicas = 2;

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            let total = state.count_owned_pods(owner);
            if total != 2 {
                return Err(format!("expected 2 pods remaining, got {total}"));
            }
            if state.pods.get(pending1).is_some() || state.pods.get(pending2).is_some() {
                return Err("pending pods should have been deleted first".into());
            }
            Ok(())
        }),
    }
}

fn deletion_cost_ordering_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::*;

    BehaviorSpec {
        name: "rs-scaledown-deletion-cost-ordering",
        description: "Pods with lower deletion_cost are deleted first",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let owner = OwnerId(1);
            let node_id = state.add_node(mk_node());

            let rs = state.add_replica_set(mk_rs(owner, 3));

            let mut high = mk_pod(owner);
            high.deletion_cost = Some(100);
            let high_id = state.submit_pod(high);
            state.bind_pod(high_id, node_id);

            let mut low = mk_pod(owner);
            low.deletion_cost = Some(-50);
            let low_id = state.submit_pod(low);
            state.bind_pod(low_id, node_id);

            let mut med = mk_pod(owner);
            med.deletion_cost = Some(10);
            let med_id = state.submit_pod(med);
            state.bind_pod(med_id, node_id);

            state.replica_sets.get_mut(rs).unwrap().desired_replicas = 1;

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            if state.count_owned_pods(owner) != 1 {
                return Err(format!("expected 1 pod, got {}", state.count_owned_pods(owner)));
            }
            if state.pods.get(high_id).is_none() {
                return Err("highest cost pod should survive".into());
            }
            if state.pods.get(low_id).is_some() {
                return Err("lowest cost pod should be deleted first".into());
            }
            Ok(())
        }),
    }
}

fn node_replica_spread_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::*;

    BehaviorSpec {
        name: "rs-scaledown-node-replica-spread",
        description: "Pods on nodes with more replicas are deleted first",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let owner = OwnerId(1);

            let crowded = state.add_node(mk_node());
            let sparse = state.add_node(mk_node());

            let rs = state.add_replica_set(mk_rs(owner, 4));

            for _ in 0..3 {
                let pid = state.submit_pod(mk_pod(owner));
                state.bind_pod(pid, crowded);
            }
            let sparse_pod = state.submit_pod(mk_pod(owner));
            state.bind_pod(sparse_pod, sparse);

            state.replica_sets.get_mut(rs).unwrap().desired_replicas = 2;

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            if state.count_owned_pods(owner) != 2 {
                return Err(format!("expected 2 pods, got {}", state.count_owned_pods(owner)));
            }
            if state.pods.get(sparse_pod).is_none() {
                return Err("pod on sparse node should survive".into());
            }
            Ok(())
        }),
    }
}

fn creation_time_recency_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::*;

    BehaviorSpec {
        name: "rs-scaledown-creation-time-recency",
        description: "More recently created pods are deleted before older pods",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let owner = OwnerId(1);
            let node_id = state.add_node(mk_node());

            let rs = state.add_replica_set(mk_rs(owner, 3));

            let old = state.submit_pod(mk_pod(owner));
            state.bind_pod(old, node_id);
            let _mid = state.submit_pod(mk_pod(owner));
            state.bind_pod(_mid, node_id);
            let new = state.submit_pod(mk_pod(owner));
            state.bind_pod(new, node_id);

            state.replica_sets.get_mut(rs).unwrap().desired_replicas = 1;

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            if state.count_owned_pods(owner) != 1 {
                return Err(format!("expected 1 pod, got {}", state.count_owned_pods(owner)));
            }
            if state.pods.get(old).is_none() {
                return Err("oldest pod should survive".into());
            }
            if state.pods.get(new).is_some() {
                return Err("newest pod should be deleted first".into());
            }
            Ok(())
        }),
    }
}

fn combined_ordering_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::*;

    BehaviorSpec {
        name: "rs-scaledown-combined-ordering",
        description: "Full priority chain: pending > deletion_cost > node spread > recency",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let owner = OwnerId(1);
            let node_id = state.add_node(mk_node());

            let rs = state.add_replica_set(mk_rs(owner, 3));

            let mut high_cost = mk_pod(owner);
            high_cost.deletion_cost = Some(100);
            let high_cost_id = state.submit_pod(high_cost);
            state.bind_pod(high_cost_id, node_id);

            let mut low_cost = mk_pod(owner);
            low_cost.deletion_cost = Some(-10);
            let low_cost_id = state.submit_pod(low_cost);
            state.bind_pod(low_cost_id, node_id);

            let pending_id = state.submit_pod(mk_pod(owner));

            state.replica_sets.get_mut(rs).unwrap().desired_replicas = 1;

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            if state.count_owned_pods(owner) != 1 {
                return Err(format!("expected 1 pod, got {}", state.count_owned_pods(owner)));
            }
            if state.pods.get(high_cost_id).is_none() {
                return Err("high cost running pod should survive".into());
            }
            if state.pods.get(pending_id).is_some() {
                return Err("pending pod should be deleted before running pods".into());
            }
            Ok(())
        }),
    }
}

fn deletion_cost_overrides_node_spread_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_engine::*;

    BehaviorSpec {
        name: "rs-scaledown-deletion-cost-overrides-spread",
        description: "Deletion cost takes priority over node replica spread",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let owner = OwnerId(1);

            let crowded = state.add_node(mk_node());
            let sparse = state.add_node(mk_node());

            let rs = state.add_replica_set(mk_rs(owner, 3));

            let mut crowded_high = mk_pod(owner);
            crowded_high.deletion_cost = Some(100);
            let crowded_high_id = state.submit_pod(crowded_high);
            state.bind_pod(crowded_high_id, crowded);

            let filler = state.submit_pod(mk_pod(owner));
            state.bind_pod(filler, crowded);

            let mut sparse_low = mk_pod(owner);
            sparse_low.deletion_cost = Some(-100);
            let sparse_low_id = state.submit_pod(sparse_low);
            state.bind_pod(sparse_low_id, sparse);

            state.replica_sets.get_mut(rs).unwrap().desired_replicas = 2;

            let mut engine = Engine::new(TimeMode::Logical);
            engine.add_handler(Box::new(ReplicaSetController));
            engine.schedule(SimTime(1), Event::ReplicaSetReconcile(owner));
            engine.run_to_completion(&mut state);

            if state.count_owned_pods(owner) != 2 {
                return Err(format!("expected 2 pods, got {}", state.count_owned_pods(owner)));
            }
            if state.pods.get(sparse_low_id).is_some() {
                return Err("low cost pod should be deleted even on sparse node".into());
            }
            if state.pods.get(crowded_high_id).is_none() {
                return Err("high cost pod should survive even on crowded node".into());
            }
            Ok(())
        }),
    }
}

fn mk_node() -> kubesim_core::Node {
    kubesim_core::Node {
        instance_type: "m5.xlarge".into(),
        allocatable: kubesim_core::Resources { cpu_millis: 8000, memory_bytes: 16_000_000_000, gpu: 0, ephemeral_bytes: 0 },
        allocated: kubesim_core::Resources::default(),
        pods: Default::default(),
        conditions: kubesim_core::NodeConditions { ready: true, ..Default::default() },
        labels: kubesim_core::LabelSet::default(),
        taints: Default::default(),
        cost_per_hour: 0.192,
        lifecycle: kubesim_core::NodeLifecycle::OnDemand,
        cordoned: false,
        created_at: kubesim_core::SimTime(0),
        pool_name: String::new(),
        do_not_disrupt: false,
    }
}

fn mk_pod(owner: kubesim_core::OwnerId) -> kubesim_core::Pod {
    kubesim_core::Pod {
        requests: kubesim_core::Resources { cpu_millis: 500, memory_bytes: 500_000_000, gpu: 0, ephemeral_bytes: 0 },
        limits: kubesim_core::Resources::default(),
        phase: kubesim_core::PodPhase::Pending,
        node: None,
        scheduling_constraints: kubesim_core::SchedulingConstraints::default(),
        deletion_cost: None,
        owner,
        qos_class: kubesim_core::QoSClass::Burstable,
        priority: 0,
        labels: kubesim_core::LabelSet::default(),
        do_not_disrupt: false,
        duration_ns: None, is_daemonset: false,
    }
}

fn mk_rs(owner: kubesim_core::OwnerId, replicas: u32) -> kubesim_core::ReplicaSet {
    kubesim_core::ReplicaSet {
        owner_id: owner,
        desired_replicas: replicas,
        pod_template: kubesim_core::PodTemplate {
            requests: kubesim_core::Resources { cpu_millis: 500, memory_bytes: 500_000_000, gpu: 0, ephemeral_bytes: 0 },
            limits: kubesim_core::Resources::default(),
            priority: 0,
            labels: kubesim_core::LabelSet::default(),
            scheduling_constraints: kubesim_core::SchedulingConstraints::default(),
        },
        deletion_cost_strategy: kubesim_core::DeletionCostStrategy::None,
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
