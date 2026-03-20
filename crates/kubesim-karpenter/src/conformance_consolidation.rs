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
        consolidation_counts_evictions_spec(),
        consolidation_respects_pdb_spec(),
        when_empty_no_evictions_spec(),
        when_empty_skips_occupied_spec(),
        cost_justified_threshold_spec(),
        decision_ratio_normalized_spec(),
        consolidation_reduces_node_count_spec(),
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
        duration_ns: None, is_daemonset: false,
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
                Some(profile), None, "default", 0, &Resources::default(), 0,
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
                Some(profile), None, "default", 0, &Resources::default(), 0,
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
                Some(profile), None, "default", 0, &Resources::default(), 0,
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
                Some(profile), None, "default", consolidate_after, &Resources::default(), 0,
            );

            if actions.len() != 1 {
                return Err(format!("expected 1 action (old node only), got {}", actions.len()));
            }
            Ok(())
        }),
    }
}

/// Spec 5: Consolidation of N pods produces N disruptions via DrainHandler.
fn consolidation_counts_evictions_spec() -> BehaviorSpec {
    use crate::consolidation::DrainHandler;
    use kubesim_core::*;
    use kubesim_engine::{Event as EngineEvent, EventHandler};

    BehaviorSpec {
        name: "consolidation-counts-evictions",
        description: "Draining a node with N pods emits N PodTerminating events",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|_profile| {
            let mut state = ClusterState::new();
            let na = state.add_node(node(4000, 8_000_000_000, "default"));
            for _ in 0..3 {
                let p = state.submit_pod(pod(500, 500_000_000));
                state.bind_pod(p, na);
            }

            let mut handler = DrainHandler;
            let events = handler.handle(&EngineEvent::NodeDrained(na), SimTime(10), &mut state);

            let terminating_count = events.iter()
                .filter(|e| matches!(e.event, EngineEvent::PodTerminating(_)))
                .count();

            if terminating_count != 3 {
                return Err(format!("expected 3 PodTerminating events, got {}", terminating_count));
            }
            Ok(())
        }),
    }
}

/// Spec 6: PDB with min_available limits evictions per drain round.
fn consolidation_respects_pdb_spec() -> BehaviorSpec {
    use crate::consolidation::DrainHandler;
    use kubesim_core::*;
    use kubesim_engine::{Event as EngineEvent, EventHandler};

    BehaviorSpec {
        name: "consolidation-respects-pdb",
        description: "PDB min_available limits how many pods are evicted per drain",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|_profile| {
            let mut state = ClusterState::new();
            let na = state.add_node(node(4000, 8_000_000_000, "default"));

            let labels = LabelSet(vec![("app".into(), "web".into())]);
            for _ in 0..3 {
                let mut p = pod(500, 500_000_000);
                p.labels = labels.clone();
                let pid = state.submit_pod(p);
                state.bind_pod(pid, na);
            }

            // PDB: min_available=2 → only 1 can be evicted at a time
            state.pdbs.push(PodDisruptionBudget {
                selector: LabelSelector { match_labels: labels.clone() },
                min_available: 2,
            });

            let mut handler = DrainHandler;
            let events = handler.handle(&EngineEvent::NodeDrained(na), SimTime(10), &mut state);

            let terminating_count = events.iter()
                .filter(|e| matches!(e.event, EngineEvent::PodTerminating(_)))
                .count();

            if terminating_count > 1 {
                return Err(format!(
                    "PDB min_available=2 with 3 pods should allow at most 1 eviction, got {}",
                    terminating_count
                ));
            }
            if terminating_count == 0 {
                return Err("expected at least 1 eviction but got 0".into());
            }
            Ok(())
        }),
    }
}

/// Spec 7: WhenEmpty on an empty node produces 0 pod disruptions.
fn when_empty_no_evictions_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "when-empty-no-evictions",
        description: "WhenEmpty on an empty node terminates it with 0 pod disruptions",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            state.add_node(node(4000, 8_000_000_000, "default"));

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenEmpty, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );

            if actions.len() != 1 {
                return Err(format!("expected 1 action, got {}", actions.len()));
            }
            match &actions[0] {
                ConsolidationAction::TerminateEmpty(_) => Ok(()),
                other => Err(format!("expected TerminateEmpty, got {:?}", other)),
            }
        }),
    }
}

/// Spec 8: WhenEmpty skips nodes that have non-daemonset pods.
fn when_empty_skips_occupied_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "when-empty-skips-occupied",
        description: "WhenEmpty does not consolidate nodes with running pods",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            let na = state.add_node(node(4000, 8_000_000_000, "default"));
            let p = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(p, na);

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenEmpty, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );

            if !actions.is_empty() {
                return Err(format!("expected 0 actions for occupied node, got {}", actions.len()));
            }
            Ok(())
        }),
    }
}

/// Spec 9: Low decision_ratio_threshold consolidates, high threshold doesn't.
fn cost_justified_threshold_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned_with_metrics, ConsolidationDecisionMetrics, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "cost-justified-threshold",
        description: "Low threshold consolidates, high threshold blocks consolidation",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            let na = state.add_node(Node { cost_per_hour: 0.5, ..node(4000, 8_000_000_000, "default") });
            let p = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(p, na);
            // Target node must have a pod so WhenEmpty doesn't pick it up
            let nb = state.add_node(node(8000, 16_000_000_000, "default"));
            let p2 = state.submit_pod(pod(100, 100_000_000));
            state.bind_pod(p2, nb);

            let mut m1 = ConsolidationDecisionMetrics::default();
            let actions_low = evaluate_versioned_with_metrics(
                &state, ConsolidationPolicy::WhenCostJustifiesDisruption, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
                0.1, Some(&mut m1),
            );

            let mut m2 = ConsolidationDecisionMetrics::default();
            let actions_high = evaluate_versioned_with_metrics(
                &state, ConsolidationPolicy::WhenCostJustifiesDisruption, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
                1000.0, Some(&mut m2),
            );

            if actions_low.is_empty() {
                return Err("low threshold should produce consolidation actions".into());
            }
            if !actions_high.is_empty() {
                return Err(format!("high threshold should block consolidation, got {} actions", actions_high.len()));
            }
            Ok(())
        }),
    }
}

/// Spec 10: Decision ratio is non-negative and finite for various pod counts.
fn decision_ratio_normalized_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned_with_metrics, ConsolidationDecisionMetrics, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "decision-ratio-normalized",
        description: "Decision ratio mean is non-negative for various pod counts",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            for pod_count in [1, 5, 20] {
                let mut state = ClusterState::new();
                let na = state.add_node(Node { cost_per_hour: 0.5, ..node(8000, 16_000_000_000, "default") });
                for _ in 0..pod_count {
                    let p = state.submit_pod(pod(100, 100_000_000));
                    state.bind_pod(p, na);
                }
                state.add_node(node(16000, 32_000_000_000, "default"));

                let mut metrics = ConsolidationDecisionMetrics::default();
                let _ = evaluate_versioned_with_metrics(
                    &state, ConsolidationPolicy::WhenCostJustifiesDisruption, 10,
                    Some(profile), None, "default", 0, &Resources::default(), 0,
                    0.0, Some(&mut metrics),
                );

                if metrics.decisions_total == 0 {
                    return Err(format!("no decisions evaluated for pod_count={}", pod_count));
                }
                let mean = metrics.decision_ratio_sum / metrics.decisions_total as f64;
                if mean < 0.0 {
                    return Err(format!("negative mean ratio {} for pod_count={}", mean, pod_count));
                }
                if mean.is_nan() || mean.is_infinite() {
                    return Err(format!("non-finite mean ratio for pod_count={}", pod_count));
                }
            }
            Ok(())
        }),
    }
}

/// Spec 11: After consolidation, fewer nodes are targeted for removal.
fn consolidation_reduces_node_count_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "consolidation-reduces-node-count",
        description: "Consolidation actions target nodes for removal, reducing node count",
        applies_to: VersionRange::from(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            let na = state.add_node(Node { cost_per_hour: 0.1, ..node(4000, 8_000_000_000, "default") });
            let p1 = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(p1, na);

            let nb = state.add_node(Node { cost_per_hour: 0.1, ..node(4000, 8_000_000_000, "default") });
            let p2 = state.submit_pod(pod(500, 500_000_000));
            state.bind_pod(p2, nb);

            state.add_node(node(8000, 16_000_000_000, "default"));

            let initial_nodes = state.nodes.len() as usize;
            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );

            let nodes_removed = actions.iter().filter(|a| matches!(
                a,
                ConsolidationAction::TerminateEmpty(_) | ConsolidationAction::DrainAndTerminate { .. }
            )).count();

            if nodes_removed == 0 {
                return Err("expected at least 1 node to be consolidated".into());
            }
            if nodes_removed >= initial_nodes {
                return Err(format!("removed {} nodes out of {} — should keep at least 1", nodes_removed, initial_nodes));
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
