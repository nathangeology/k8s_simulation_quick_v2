//! Conformance specs for kube-scheduler filtering and scoring behaviors.

use crate::conformance::{BehaviorSpec, VersionRange};

/// Returns all scheduler conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        node_resources_fit_spec(),
        node_affinity_spec(),
        taint_toleration_spec(),
        topology_spread_spec(),
        most_allocated_spec(),
        least_allocated_spec(),
        score_tiebreaking_spec(),
        pending_fifo_spec(),
        unschedulable_stays_pending_spec(),
    ]
}

/// Spec 1: NodeResourcesFit rejects pods when node lacks CPU/memory.
fn node_resources_fit_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-node-resources-fit",
        description: "Pod is rejected if node doesn't have enough CPU/memory",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            state.add_node(mk_node(1000, 1_000_000_000));
            let pid = state.submit_pod(mk_pod(2000, 1_000_000_000));

            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            let (bound, unsched) = sched.schedule_pending(&mut state);

            if bound != 0 || unsched != 1 {
                return Err(format!("expected 0 bound / 1 unsched, got {bound}/{unsched}"));
            }
            if state.pods.get(pid).unwrap().phase != PodPhase::Pending {
                return Err("pod should remain Pending".into());
            }
            Ok(())
        }),
    }
}

/// Spec 2: NodeAffinity required terms filter non-matching nodes.
fn node_affinity_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-node-affinity-required",
        description: "Pod with required node affinity only schedules on matching nodes",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();

            // Non-matching node
            state.add_node(mk_node(4000, 8_000_000_000));

            // Matching node
            let mut matching = mk_node(4000, 8_000_000_000);
            matching.labels.insert("zone".into(), "us-east-1a".into());
            let match_nid = state.add_node(matching);

            let mut pod = mk_pod(500, 500_000_000);
            pod.scheduling_constraints.node_affinity.push(NodeAffinityTerm {
                affinity_type: AffinityType::Required,
                match_labels: LabelSet(vec![("zone".into(), "us-east-1a".into())]),
            });
            let pid = state.submit_pod(pod);

            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            let (bound, _) = sched.schedule_pending(&mut state);

            if bound != 1 {
                return Err(format!("expected 1 bound, got {bound}"));
            }
            if state.pods.get(pid).unwrap().node != Some(match_nid) {
                return Err("pod should be on the matching node".into());
            }
            Ok(())
        }),
    }
}

/// Spec 3: TaintToleration filters nodes with untolerated taints.
fn taint_toleration_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-taint-toleration",
        description: "Pod without matching toleration is filtered from tainted nodes",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            let mut tainted = mk_node(4000, 8_000_000_000);
            tainted.taints.push(Taint {
                key: "gpu".into(),
                value: "true".into(),
                effect: TaintEffect::NoSchedule,
            });
            state.add_node(tainted);

            // Clean node
            let clean_nid = state.add_node(mk_node(4000, 8_000_000_000));

            let pid = state.submit_pod(mk_pod(500, 500_000_000));

            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            let (bound, _) = sched.schedule_pending(&mut state);

            if bound != 1 {
                return Err(format!("expected 1 bound, got {bound}"));
            }
            if state.pods.get(pid).unwrap().node != Some(clean_nid) {
                return Err("pod should land on the untainted node".into());
            }
            Ok(())
        }),
    }
}

/// Spec 4: PodTopologySpread respects maxSkew with DoNotSchedule.
fn topology_spread_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-topology-spread-max-skew",
        description: "Pods respect maxSkew topology spread constraints",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();

            let selector = LabelSelector {
                match_labels: LabelSet(vec![("app".into(), "web".into())]),
            };

            // Two zones, each with a node
            let mut zone_a = mk_node(4000, 8_000_000_000);
            zone_a.labels.insert("zone".into(), "a".into());
            let nid_a = state.add_node(zone_a);

            let mut zone_b = mk_node(4000, 8_000_000_000);
            zone_b.labels.insert("zone".into(), "b".into());
            let nid_b = state.add_node(zone_b);

            // Place one existing pod in zone-a
            let mut existing = mk_pod(100, 100_000_000);
            existing.labels.insert("app".into(), "web".into());
            let eid = state.submit_pod(existing);
            state.bind_pod(eid, nid_a);

            // New pod with maxSkew=1 should go to zone-b (skew would be 2 on zone-a)
            let mut pod = mk_pod(100, 100_000_000);
            pod.labels.insert("app".into(), "web".into());
            pod.scheduling_constraints.topology_spread.push(TopologySpreadConstraint {
                max_skew: 1,
                topology_key: "zone".into(),
                when_unsatisfiable: WhenUnsatisfiable::DoNotSchedule,
                label_selector: selector,
            });
            let pid = state.submit_pod(pod);

            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            let (bound, _) = sched.schedule_pending(&mut state);

            if bound != 1 {
                return Err(format!("expected 1 bound, got {bound}"));
            }
            if state.pods.get(pid).unwrap().node != Some(nid_b) {
                return Err("pod should land in zone-b to satisfy maxSkew=1".into());
            }
            Ok(())
        }),
    }
}

/// Spec 5: MostAllocated prefers the fuller node (bin-packing).
fn most_allocated_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-most-allocated",
        description: "MostAllocated scoring prefers nodes with highest utilization",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();

            // Fuller node (75%)
            let full_nid = state.add_node(Node {
                allocated: Resources { cpu_millis: 3000, memory_bytes: 6_000_000_000, gpu: 0, ephemeral_bytes: 0 },
                ..mk_node(4000, 8_000_000_000)
            });
            // Emptier node (25%)
            state.add_node(Node {
                allocated: Resources { cpu_millis: 1000, memory_bytes: 2_000_000_000, gpu: 0, ephemeral_bytes: 0 },
                ..mk_node(4000, 8_000_000_000)
            });

            let pid = state.submit_pod(mk_pod(500, 500_000_000));
            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::MostAllocated));
            sched.schedule_pending(&mut state);

            if state.pods.get(pid).unwrap().node != Some(full_nid) {
                return Err("MostAllocated should prefer the fuller node".into());
            }
            Ok(())
        }),
    }
}

/// Spec 6: LeastAllocated prefers the emptier node (spreading).
fn least_allocated_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-least-allocated",
        description: "LeastAllocated scoring prefers nodes with lowest utilization",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();

            // Fuller node (75%)
            state.add_node(Node {
                allocated: Resources { cpu_millis: 3000, memory_bytes: 6_000_000_000, gpu: 0, ephemeral_bytes: 0 },
                ..mk_node(4000, 8_000_000_000)
            });
            // Emptier node (25%)
            let empty_nid = state.add_node(Node {
                allocated: Resources { cpu_millis: 1000, memory_bytes: 2_000_000_000, gpu: 0, ephemeral_bytes: 0 },
                ..mk_node(4000, 8_000_000_000)
            });

            let pid = state.submit_pod(mk_pod(500, 500_000_000));
            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            sched.schedule_pending(&mut state);

            if state.pods.get(pid).unwrap().node != Some(empty_nid) {
                return Err("LeastAllocated should prefer the emptier node".into());
            }
            Ok(())
        }),
    }
}

/// Spec 7: When scores are equal, selection is deterministic across runs.
fn score_tiebreaking_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-score-tiebreaking-deterministic",
        description: "When scores are equal, node selection is deterministic",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            // Run scheduling 5 times with identical state; result must be the same.
            let mut results = Vec::new();
            for _ in 0..5 {
                let mut state = ClusterState::new();
                state.add_node(mk_node(4000, 8_000_000_000));
                state.add_node(mk_node(4000, 8_000_000_000));
                let pid = state.submit_pod(mk_pod(500, 500_000_000));

                let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
                sched.schedule_pending(&mut state);
                results.push(state.pods.get(pid).unwrap().node);
            }
            if results.windows(2).any(|w| w[0] != w[1]) {
                return Err(format!("non-deterministic tiebreaking: {:?}", results));
            }
            Ok(())
        }),
    }
}

/// Spec 8: Pending pods are scheduled in priority order (highest first).
fn pending_fifo_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-pending-priority-order",
        description: "Pending pods are scheduled highest-priority first from the queue",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            // Only room for one pod
            state.add_node(mk_node(1000, 1_000_000_000));

            let mut low = mk_pod(1000, 1_000_000_000);
            low.priority = 0;
            let low_id = state.submit_pod(low);

            let mut high = mk_pod(1000, 1_000_000_000);
            high.priority = 100;
            let high_id = state.submit_pod(high);

            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            let (bound, unsched) = sched.schedule_pending(&mut state);

            if bound != 1 || unsched != 1 {
                return Err(format!("expected 1 bound / 1 unsched, got {bound}/{unsched}"));
            }
            if state.pods.get(high_id).unwrap().phase != PodPhase::Running {
                return Err("high-priority pod should be scheduled".into());
            }
            if state.pods.get(low_id).unwrap().phase != PodPhase::Pending {
                return Err("low-priority pod should remain pending".into());
            }
            Ok(())
        }),
    }
}

/// Spec 9: Unschedulable pods remain in the pending queue (not dropped).
fn unschedulable_stays_pending_spec() -> BehaviorSpec {
    use kubesim_core::*;
    use kubesim_scheduler::*;

    BehaviorSpec {
        name: "scheduler-unschedulable-stays-pending",
        description: "Unschedulable pods remain in pending queue, not silently dropped",
        applies_to: VersionRange::all(),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            // No nodes at all
            let pid = state.submit_pod(mk_pod(1000, 1_000_000_000));

            let mut sched = Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated));
            let (bound, unsched) = sched.schedule_pending(&mut state);

            if bound != 0 || unsched != 1 {
                return Err(format!("expected 0 bound / 1 unsched, got {bound}/{unsched}"));
            }
            let pod = state.pods.get(pid).unwrap();
            if pod.phase != PodPhase::Pending {
                return Err(format!("pod should be Pending, got {:?}", pod.phase));
            }
            // Pod must still exist in the arena (not removed)
            if state.pods.get(pid).is_none() {
                return Err("unschedulable pod was removed from state".into());
            }
            Ok(())
        }),
    }
}

// ── Helpers ─────────────────────────────────────────────────────

fn mk_node(cpu: u64, mem: u64) -> kubesim_core::Node {
    kubesim_core::Node {
        instance_type: "m5.xlarge".into(),
        allocatable: kubesim_core::Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
        allocated: kubesim_core::Resources::default(),
        pods: smallvec::smallvec![],
        conditions: kubesim_core::NodeConditions { ready: true, ..Default::default() },
        labels: kubesim_core::LabelSet::default(),
        taints: smallvec::smallvec![],
        cost_per_hour: 0.192,
        lifecycle: kubesim_core::NodeLifecycle::OnDemand,
        cordoned: false,
        created_at: kubesim_core::SimTime(0),
        pool_name: String::new(),
        do_not_disrupt: false,
    }
}

fn mk_pod(cpu: u64, mem: u64) -> kubesim_core::Pod {
    kubesim_core::Pod {
        requests: kubesim_core::Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
        limits: kubesim_core::Resources::default(),
        phase: kubesim_core::PodPhase::Pending,
        node: None,
        scheduling_constraints: kubesim_core::SchedulingConstraints::default(),
        deletion_cost: None,
        owner: kubesim_core::OwnerId(0),
        qos_class: kubesim_core::QoSClass::Burstable,
        priority: 0,
        labels: kubesim_core::LabelSet::default(),
        do_not_disrupt: false,
        duration_ns: None, is_daemonset: false,
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
