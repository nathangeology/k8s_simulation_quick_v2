//! Conformance specs for consolidation behaviors.

use crate::conformance::{BehaviorSpec, VersionRange};
use crate::version::KarpenterVersion;

/// Returns all consolidation conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        prefer_emptying_nodes_spec(),
        empty_before_underutilized_spec(),
        greedy_excludes_already_selected_spec(),
        consolidate_after_exempts_young_nodes_spec(),
    ]
}

fn node(cpu: u64, mem: u64, pool: &str) -> kubesim_core::Node {
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
        pool_name: pool.into(),
        do_not_disrupt: false,
    }
}

fn pod(cpu: u64, mem: u64) -> kubesim_core::Pod {
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
        duration_ns: None,
    }
}

/// Spec 1: candidate_score sorts by pod count (lower = better candidate).
fn prefer_emptying_nodes_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use crate::version::VersionProfile;
    use kubesim_core::*;

    BehaviorSpec {
        name: "prefer-emptying-nodes-sorts-by-pod-count",
        description: "PreferEmptyingNodes sorts candidates by pod count, not node capacity",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();

            let na = state.add_node(node(4000, 8_000_000_000, "default"));
            let p1 = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(p1, na);

            let nb = state.add_node(node(4000, 8_000_000_000, "default"));
            for _ in 0..3 {
                let p = state.submit_pod(pod(200, 200_000_000));
                state.bind_pod(p, nb);
            }

            state.add_node(node(8000, 16_000_000_000, "default"));

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0,
            );

            let drain_ids: Vec<NodeId> = actions.iter().filter_map(|a| match a {
                ConsolidationAction::DrainAndTerminate { node_id, .. } => Some(*node_id),
                _ => None,
            }).collect();

            if drain_ids.is_empty() {
                return Err("expected drain actions but got none".into());
            }
            if drain_ids[0] != na {
                return Err(format!(
                    "expected node with fewer pods (na) to be first candidate, got {:?}",
                    drain_ids[0]
                ));
            }
            Ok(())
        }),
    }
}

/// Spec 2: Empty-node consolidation fires before underutilized-node consolidation.
fn empty_before_underutilized_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "empty-node-before-underutilized",
        description: "Empty-node consolidation fires before underutilized-node consolidation",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();

            state.add_node(node(4000, 8_000_000_000, "default"));

            let nu = state.add_node(node(4000, 8_000_000_000, "default"));
            let p = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(p, nu);

            state.add_node(node(8000, 16_000_000_000, "default"));

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0,
            );

            if actions.is_empty() {
                return Err("expected consolidation actions".into());
            }

            let first_empty = actions.iter().position(|a| matches!(a, ConsolidationAction::TerminateEmpty(_)));
            let first_drain = actions.iter().position(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. }));

            match (first_empty, first_drain) {
                (Some(e), Some(d)) if e < d => Ok(()),
                (Some(_), None) => Ok(()),
                (None, Some(_)) => Err("empty node should have been terminated before drain".into()),
                (Some(e), Some(d)) => Err(format!("empty at index {} but drain at index {}", e, d)),
                (None, None) => Err("no consolidation actions produced".into()),
            }
        }),
    }
}

/// Spec 3: Greedy candidate selection excludes nodes already selected for removal.
fn greedy_excludes_already_selected_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "greedy-excludes-already-selected",
        description: "Greedy candidate selection excludes nodes already selected for removal",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();

            let na = state.add_node(node(4000, 8_000_000_000, "default"));
            let pa = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(pa, na);

            let nb = state.add_node(node(4000, 8_000_000_000, "default"));
            let pb = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(pb, nb);

            let nc = state.add_node(node(4000, 8_000_000_000, "default"));
            let pc = state.submit_pod(pod(3000, 7_000_000_000));
            state.bind_pod(pc, nc);

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0,
            );

            let drain_count = actions.iter().filter(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. })).count();
            if drain_count > 1 {
                return Err(format!("expected at most 1 drain (greedy exclusion), got {}", drain_count));
            }
            if drain_count == 0 {
                return Err("expected 1 drain action but got 0".into());
            }
            Ok(())
        }),
    }
}

/// Spec 4: ConsolidateAfter exempts recently-created nodes from consolidation.
fn consolidate_after_exempts_young_nodes_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "consolidate-after-exempts-young-nodes",
        description: "ConsolidateAfter exempts recently-created nodes from consolidation",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            state.time = SimTime(100_000_000_000);

            let mut young = node(4000, 8_000_000_000, "default");
            young.created_at = SimTime(90_000_000_000);
            state.add_node(young);

            state.add_node(node(4000, 8_000_000_000, "default"));

            let consolidate_after = 30_000_000_000u64;
            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenEmpty, 10,
                Some(profile), None, "default", consolidate_after,
            );

            if actions.len() != 1 {
                return Err(format!("expected 1 action (old node only), got {}", actions.len()));
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
