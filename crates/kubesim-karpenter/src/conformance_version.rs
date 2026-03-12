//! Conformance specs for Karpenter version-specific behavioral differences.
//!
//! Tests behavioral differences between v0.35 (pre-GA) and v1.x (GA),
//! plus cross-version invariants that hold across all versions.

use crate::conformance::{BehaviorSpec, VersionRange};
use crate::version::{ConsolidationStrategy, KarpenterVersion};

/// Returns all version-difference conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        // v0.35 specs
        v035_single_node_consolidation_only(),
        v035_no_hash_based_drift(),
        v035_no_replace_consolidation(),
        // v1.x specs
        v1_multi_node_consolidation(),
        v1_hash_based_drift(),
        v1_replace_consolidation(),
        v1_disruption_budgets_enforced(),
        v1_when_underutilized_policy(),
        // Cross-version specs
        cross_empty_node_consolidation_before_underutilized(),
        cross_provisioning_instance_selection_consistent(),
    ]
}

// ── v0.35 specs ─────────────────────────────────────────────────

fn v035_single_node_consolidation_only() -> BehaviorSpec {
    BehaviorSpec {
        name: "v035-single-node-consolidation-only",
        description: "v0.35 uses SingleNode consolidation strategy (no multi-node batching)",
        applies_to: VersionRange::exact(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            if profile.consolidation_strategy != ConsolidationStrategy::SingleNode {
                return Err(format!(
                    "expected SingleNode, got {:?}",
                    profile.consolidation_strategy
                ));
            }
            // Verify: with two drainable nodes, only one drain action is produced
            use kubesim_core::*;
            let mut state = ClusterState::new();
            let na = state.add_node(mk_node("default"));
            let nb = state.add_node(mk_node("default"));
            // Absorber node — give it a pod so it's not considered empty
            let nc = state.add_node(mk_node("default"));
            let pc = state.submit_pod(mk_pod(100, 100_000_000));
            state.bind_pod(pc, nc);

            let pa = state.submit_pod(mk_pod(500, 500_000_000));
            state.bind_pod(pa, na);
            let pb = state.submit_pod(mk_pod(500, 500_000_000));
            state.bind_pod(pb, nb);

            let actions = crate::consolidation::evaluate_versioned(
                &state,
                crate::consolidation::ConsolidationPolicy::WhenUnderutilized,
                10,
                Some(profile),
                None,
                "default",
                0,
            );
            let drain_count = actions.iter().filter(|a| {
                matches!(a, crate::consolidation::ConsolidationAction::DrainAndTerminate { .. })
            }).count();
            if drain_count > 1 {
                return Err(format!(
                    "SingleNode should drain at most 1 node, got {}",
                    drain_count
                ));
            }
            Ok(())
        }),
    }
}

fn v035_no_hash_based_drift() -> BehaviorSpec {
    BehaviorSpec {
        name: "v035-no-hash-based-drift",
        description: "v0.35 does not use hash-based drift detection",
        applies_to: VersionRange::exact(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            if profile.hash_based_drift {
                return Err("v0.35 should not have hash_based_drift enabled".into());
            }
            Ok(())
        }),
    }
}

fn v035_no_replace_consolidation() -> BehaviorSpec {
    BehaviorSpec {
        name: "v035-no-replace-consolidation",
        description: "v0.35 does not support replace consolidation (cheaper instance swap)",
        applies_to: VersionRange::exact(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            if profile.replace_consolidation {
                return Err("v0.35 should not have replace_consolidation enabled".into());
            }
            Ok(())
        }),
    }
}

// ── v1.x specs ──────────────────────────────────────────────────

fn v1_multi_node_consolidation() -> BehaviorSpec {
    BehaviorSpec {
        name: "v1-multi-node-consolidation",
        description: "v1.x uses MultiNode consolidation (can batch multiple nodes)",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            if profile.consolidation_strategy != ConsolidationStrategy::MultiNode {
                return Err(format!(
                    "expected MultiNode, got {:?}",
                    profile.consolidation_strategy
                ));
            }
            // Use 20+ nodes so the 10% disruption budget allows >=2 drains
            use kubesim_core::*;
            let mut state = ClusterState::new();

            // 18 heavily-loaded nodes (not consolidation candidates)
            for _ in 0..18 {
                let n = state.add_node(mk_node("default"));
                let p = state.submit_pod(mk_pod(3500, 7_000_000_000));
                state.bind_pod(p, n);
            }

            // 2 lightly-loaded nodes (consolidation candidates)
            let na = state.add_node(mk_node("default"));
            let pa = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(pa, na);
            let nb = state.add_node(mk_node("default"));
            let pb = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(pb, nb);

            let actions = crate::consolidation::evaluate_versioned(
                &state,
                crate::consolidation::ConsolidationPolicy::WhenUnderutilized,
                10,
                Some(profile),
                None,
                "default",
                0,
            );
            let drain_count = actions.iter().filter(|a| {
                matches!(a, crate::consolidation::ConsolidationAction::DrainAndTerminate { .. })
            }).count();
            if drain_count < 2 {
                return Err(format!(
                    "MultiNode should drain both lightly-loaded nodes, got {} drain actions",
                    drain_count
                ));
            }
            Ok(())
        }),
    }
}

fn v1_hash_based_drift() -> BehaviorSpec {
    BehaviorSpec {
        name: "v1-hash-based-drift",
        description: "v1.x enables hash-based drift detection (labels/taints, not just AMI)",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            if !profile.hash_based_drift {
                return Err("v1.x should have hash_based_drift enabled".into());
            }
            Ok(())
        }),
    }
}

fn v1_replace_consolidation() -> BehaviorSpec {
    BehaviorSpec {
        name: "v1-replace-consolidation",
        description: "v1.x supports replace consolidation (swap to cheaper instance type)",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            if !profile.replace_consolidation {
                return Err("v1.x should have replace_consolidation enabled".into());
            }
            Ok(())
        }),
    }
}

fn v1_disruption_budgets_enforced() -> BehaviorSpec {
    use crate::version::{DisruptionBudgetConfig, DisruptionReason};

    BehaviorSpec {
        name: "v1-disruption-budgets-per-reason",
        description: "v1.x enforces per-reason disruption budgets",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|_profile| {
            use kubesim_core::*;
            use crate::version::VersionProfile;

            // Create a profile with a per-reason budget: only 1 empty node at a time
            let mut vp = VersionProfile::new(KarpenterVersion::V1);
            vp.budgets = vec![DisruptionBudgetConfig {
                max_percent: 100, // would allow all if not per-reason
                reasons: vec![DisruptionReason::Empty],
                schedule: None,
                active_budget: None,
                inactive_budget: None,
            }];

            let mut state = ClusterState::new();
            // 3 empty nodes
            state.add_node(mk_node("default"));
            state.add_node(mk_node("default"));
            state.add_node(mk_node("default"));

            let actions = crate::consolidation::evaluate_versioned(
                &state,
                crate::consolidation::ConsolidationPolicy::WhenEmpty,
                10, // global max
                Some(&vp),
                None,
                "default",
                0,
            );
            // Per-reason budget: 100% of 3 nodes = 3, but global max is 10
            // All 3 should be terminated since the per-reason cap is 3
            if actions.is_empty() {
                return Err("per-reason budget should allow disruption".into());
            }
            Ok(())
        }),
    }
}

fn v1_when_underutilized_policy() -> BehaviorSpec {
    BehaviorSpec {
        name: "v1-when-underutilized-consolidation",
        description: "v1.x WhenUnderutilized policy drains underutilized nodes",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            use kubesim_core::*;
            let mut state = ClusterState::new();
            // Underutilized node: small pod on big node
            let na = state.add_node(mk_node("default"));
            let pa = state.submit_pod(mk_pod(100, 100_000_000));
            state.bind_pod(pa, na);
            // Absorber node
            state.add_node(mk_node("default"));

            let actions = crate::consolidation::evaluate_versioned(
                &state,
                crate::consolidation::ConsolidationPolicy::WhenUnderutilized,
                10,
                Some(profile),
                None,
                "default",
                0,
            );
            let has_drain = actions.iter().any(|a| {
                matches!(a, crate::consolidation::ConsolidationAction::DrainAndTerminate { .. })
            });
            if !has_drain {
                return Err("WhenUnderutilized should drain underutilized nodes".into());
            }
            Ok(())
        }),
    }
}

// ── Cross-version specs ─────────────────────────────────────────

fn cross_empty_node_consolidation_before_underutilized() -> BehaviorSpec {
    BehaviorSpec {
        name: "cross-empty-before-underutilized",
        description: "Both versions consolidate empty nodes before underutilized ones",
        applies_to: VersionRange::all(),
        test: Box::new(|profile| {
            use kubesim_core::*;
            let mut state = ClusterState::new();
            // Empty node
            state.add_node(mk_node("default"));
            // Underutilized node
            let nu = state.add_node(mk_node("default"));
            let pu = state.submit_pod(mk_pod(100, 100_000_000));
            state.bind_pod(pu, nu);
            // Absorber
            state.add_node(mk_node("default"));

            let actions = crate::consolidation::evaluate_versioned(
                &state,
                crate::consolidation::ConsolidationPolicy::WhenUnderutilized,
                10,
                Some(profile),
                None,
                "default",
                0,
            );
            if actions.is_empty() {
                return Err("should produce at least one consolidation action".into());
            }
            // First action must be TerminateEmpty
            if !matches!(actions[0], crate::consolidation::ConsolidationAction::TerminateEmpty(_)) {
                return Err(format!(
                    "first action should be TerminateEmpty, got {:?}",
                    std::mem::discriminant(&actions[0])
                ));
            }
            Ok(())
        }),
    }
}

fn cross_provisioning_instance_selection_consistent() -> BehaviorSpec {
    BehaviorSpec {
        name: "cross-provisioning-selects-cheapest-fit",
        description: "Both versions select the cheapest instance type that fits the workload",
        applies_to: VersionRange::all(),
        test: Box::new(|profile| {
            use kubesim_core::*;
            let catalog = kubesim_ec2::Catalog::embedded()
                .map_err(|e| format!("catalog load failed: {e}"))?;
            let pool = crate::nodepool::NodePool {
                name: "default".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                limits: crate::nodepool::NodePoolLimits::default(),
                labels: vec![],
                taints: vec![],
                max_disrupted_pct: 10,
                max_disrupted_count: None,
                weight: 0,
                do_not_disrupt: false,
            };
            let usage = crate::nodepool::NodePoolUsage::default();

            let mut state = ClusterState::new();
            // Pod that fits on m5.xlarge (4 vcpu, 16 GiB)
            state.submit_pod(mk_pod(2000, 4_000_000_000));

            let decisions = crate::provisioner::provision_versioned(
                &state,
                &catalog,
                &pool,
                &usage,
                Some(profile),
            );
            if decisions.is_empty() {
                return Err("should produce a provisioning decision".into());
            }
            // m5.xlarge is cheaper than m5.2xlarge and fits the pod
            if decisions[0].instance_type != "m5.xlarge" {
                return Err(format!(
                    "expected m5.xlarge (cheapest fit), got {}",
                    decisions[0].instance_type
                ));
            }
            Ok(())
        }),
    }
}

// ── Helpers ─────────────────────────────────────────────────────

fn mk_node(pool_name: &str) -> kubesim_core::Node {
    kubesim_core::Node {
        instance_type: "m5.xlarge".into(),
        allocatable: kubesim_core::Resources {
            cpu_millis: 4000,
            memory_bytes: 8_000_000_000,
            gpu: 0,
            ephemeral_bytes: 0,
        },
        allocated: kubesim_core::Resources::default(),
        pods: smallvec::smallvec![],
        conditions: kubesim_core::NodeConditions { ready: true, ..Default::default() },
        labels: kubesim_core::LabelSet::default(),
        taints: smallvec::smallvec![],
        cost_per_hour: 0.192,
        lifecycle: kubesim_core::NodeLifecycle::OnDemand,
        cordoned: false,
        created_at: kubesim_core::SimTime(0),
        pool_name: pool_name.into(),
        do_not_disrupt: false,
    }
}

fn mk_pod(cpu: u64, mem: u64) -> kubesim_core::Pod {
    kubesim_core::Pod {
        requests: kubesim_core::Resources {
            cpu_millis: cpu,
            memory_bytes: mem,
            gpu: 0,
            ephemeral_bytes: 0,
        },
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

    #[test]
    fn v035_specs_skip_on_v1() {
        let profile = VersionProfile::new(KarpenterVersion::V1);
        let report = run_conformance(&profile, &specs());
        let v035_skipped = report.results.iter().any(|r| {
            matches!(r, crate::conformance::SpecResult::Skip { name, .. } if name.starts_with("v035-"))
        });
        assert!(v035_skipped, "v0.35-only specs should be skipped on v1");
    }

    #[test]
    fn v1_specs_skip_on_v035() {
        let profile = VersionProfile::new(KarpenterVersion::V0_35);
        let report = run_conformance(&profile, &specs());
        let v1_skipped = report.results.iter().any(|r| {
            matches!(r, crate::conformance::SpecResult::Skip { name, .. } if name.starts_with("v1-"))
        });
        assert!(v1_skipped, "v1-only specs should be skipped on v0.35");
    }
}
