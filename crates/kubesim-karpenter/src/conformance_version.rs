//! Conformance specs for Karpenter version-specific behavioral differences.

use crate::conformance::{BehaviorSpec, VersionRange};
use crate::version::KarpenterVersion;

/// Returns all version-difference conformance specs.
pub fn specs() -> Vec<BehaviorSpec> {
    vec![
        v035_single_node_consolidation_spec(),
        v1_multi_node_consolidation_spec(),
        v1_when_underutilized_policy_spec(),
        v1_disruption_budgets_enforced_spec(),
        v035_no_hash_drift_spec(),
        v1_hash_based_drift_spec(),
        v035_no_replace_consolidation_spec(),
        v1_replace_consolidation_spec(),
        cross_version_empty_node_first_spec(),
        cross_version_provisioning_cheapest_fit_spec(),
    ]
}

/// v0.35: consolidation removes at most one underutilized node per pass (single-node).
fn v035_single_node_consolidation_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "v035-single-node-consolidation",
        description: "v0.35 consolidation drains at most one underutilized node per pass",
        applies_to: VersionRange::exact(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            // Target node with capacity (has a pod so it's not "empty")
            let target = state.add_node(mk_node_pool(8000, 16_000_000_000, "default"));
            let anchor = state.submit_pod(mk_pod(100, 100_000_000));
            state.bind_pod(anchor, target);

            // Two underutilized nodes with small pods
            let n1 = state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            let p1 = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(p1, n1);

            let n2 = state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            let p2 = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(p2, n2);

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );
            let drain_count = actions.iter()
                .filter(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. }))
                .count();
            if drain_count > 1 {
                return Err(format!("v0.35 should drain at most 1 node, got {drain_count}"));
            }
            Ok(())
        }),
    }
}

/// v1.0: consolidation can remove multiple underutilized nodes in a single pass.
fn v1_multi_node_consolidation_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "v1-multi-node-consolidation",
        description: "v1.0 consolidation can drain multiple underutilized nodes per pass",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            // Target node marked do-not-disrupt so it absorbs pods without being
            // a consolidation candidate itself.
            let target = state.add_node(Node {
                do_not_disrupt: true,
                ..mk_node_pool(8000, 16_000_000_000, "default")
            });
            let anchor = state.submit_pod(mk_pod(100, 100_000_000));
            state.bind_pod(anchor, target);

            // Two underutilized nodes with small pods
            let n1 = state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            let p1 = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(p1, n1);

            let n2 = state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            let p2 = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(p2, n2);

            // Filler nodes in a separate pool to raise total_nodes so the 10%
            // disruption budget allows ≥2 underutilized disruptions.
            for _ in 0..17 {
                let fid = state.add_node(mk_node_pool(1000, 1_000_000_000, "filler"));
                let fp = state.submit_pod(mk_pod(100, 100_000_000));
                state.bind_pod(fp, fid);
            }

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );
            let drain_count = actions.iter()
                .filter(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. }))
                .count();
            if drain_count < 2 {
                return Err(format!("v1.0 should drain multiple nodes, got {drain_count}"));
            }
            Ok(())
        }),
    }
}

/// v1.0: WhenUnderutilized policy identifies nodes whose pods fit elsewhere.
fn v1_when_underutilized_policy_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "v1-when-underutilized-policy",
        description: "v1.0 WhenUnderutilized drains nodes whose pods can be rescheduled",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            // Big node that can absorb the small pod
            state.add_node(mk_node_pool(8000, 16_000_000_000, "default"));

            // Underutilized node
            let small = state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            let pid = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(pid, small);

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );
            let has_drain = actions.iter().any(|a| match a {
                ConsolidationAction::DrainAndTerminate { node_id, .. } => *node_id == small,
                _ => false,
            });
            if !has_drain {
                return Err("WhenUnderutilized should drain the underutilized node".into());
            }
            Ok(())
        }),
    }
}

/// v1.0: disruption budgets limit how many nodes can be disrupted per pass.
fn v1_disruption_budgets_enforced_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationPolicy};
    use crate::version::{DisruptionBudgetConfig, DisruptionReason, VersionProfile};
    use kubesim_core::*;

    BehaviorSpec {
        name: "v1-disruption-budgets-enforced",
        description: "v1.0 per-reason disruption budgets limit consolidation actions",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|_| {
            let mut state = ClusterState::new();
            // 5 empty nodes
            for _ in 0..5 {
                state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            }

            // Budget: only 20% of empty nodes can be disrupted (= 1 of 5)
            let mut profile = VersionProfile::new(KarpenterVersion::V1);
            profile.budgets = vec![DisruptionBudgetConfig {
                max_percent: 20,
                reasons: vec![DisruptionReason::Empty],
                schedule: None,
                active_budget: None,
                inactive_budget: None,
            }];

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenEmpty, 10,
                Some(&profile), None, "default", 0, &Resources::default(), 0,
            );
            if actions.len() > 1 {
                return Err(format!("budget should limit to 1 empty termination, got {}", actions.len()));
            }
            Ok(())
        }),
    }
}

/// v0.35: drift detection is instance-type only (no hash-based label drift).
fn v035_no_hash_drift_spec() -> BehaviorSpec {
    use crate::drift::{DriftConfig, DriftHandler};
    use crate::nodepool::NodePool;

    BehaviorSpec {
        name: "v035-no-hash-based-drift",
        description: "v0.35 does not detect label drift (only instance type drift)",
        applies_to: VersionRange::exact(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.xlarge".into()],
                labels: vec![("env".into(), "prod".into())],
                ..mk_pool()
            };
            let handler = DriftHandler::new(pool, DriftConfig::default())
                .with_version(profile.clone());

            // Node has correct instance type but wrong labels
            let mut node = mk_node(4000, 8_000_000_000);
            node.instance_type = "m5.xlarge".into();
            node.labels.insert("env".into(), "staging".into());

            if handler.is_drifted(&node) {
                return Err("v0.35 should NOT detect label drift".into());
            }
            Ok(())
        }),
    }
}

/// v1.0: drift detection includes hash-based label comparison.
fn v1_hash_based_drift_spec() -> BehaviorSpec {
    use crate::drift::{DriftConfig, DriftHandler};
    use crate::nodepool::NodePool;

    BehaviorSpec {
        name: "v1-hash-based-drift",
        description: "v1.0 detects label drift via hash-based comparison",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.xlarge".into()],
                labels: vec![("env".into(), "prod".into())],
                ..mk_pool()
            };
            let handler = DriftHandler::new(pool, DriftConfig::default())
                .with_version(profile.clone());

            // Node has correct instance type but wrong labels
            let mut node = mk_node(4000, 8_000_000_000);
            node.instance_type = "m5.xlarge".into();
            node.labels.insert("env".into(), "staging".into());

            if !handler.is_drifted(&node) {
                return Err("v1.0 should detect label drift".into());
            }
            Ok(())
        }),
    }
}

/// v0.35: replace consolidation is not available.
fn v035_no_replace_consolidation_spec() -> BehaviorSpec {
    BehaviorSpec {
        name: "v035-no-replace-consolidation",
        description: "v0.35 profile has replace_consolidation disabled",
        applies_to: VersionRange::exact(KarpenterVersion::V0_35),
        test: Box::new(|profile| {
            if profile.replace_consolidation {
                return Err("v0.35 should not support replace consolidation".into());
            }
            Ok(())
        }),
    }
}

/// v1.0: replace consolidation is available.
fn v1_replace_consolidation_spec() -> BehaviorSpec {
    BehaviorSpec {
        name: "v1-replace-consolidation",
        description: "v1.0 profile has replace_consolidation enabled",
        applies_to: VersionRange::exact(KarpenterVersion::V1),
        test: Box::new(|profile| {
            if !profile.replace_consolidation {
                return Err("v1.0 should support replace consolidation".into());
            }
            Ok(())
        }),
    }
}

/// Both versions: empty nodes are consolidated before underutilized nodes.
fn cross_version_empty_node_first_spec() -> BehaviorSpec {
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use kubesim_core::*;

    BehaviorSpec {
        name: "cross-version-empty-before-underutilized",
        description: "Both versions consolidate empty nodes before underutilized ones",
        applies_to: VersionRange::all(),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            // Target node with capacity
            state.add_node(mk_node_pool(8000, 16_000_000_000, "default"));

            // Empty node
            state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));

            // Underutilized node
            let under = state.add_node(mk_node_pool(4000, 8_000_000_000, "default"));
            let pid = state.submit_pod(mk_pod(200, 200_000_000));
            state.bind_pod(pid, under);

            let actions = evaluate_versioned(
                &state, ConsolidationPolicy::WhenUnderutilized, 10,
                Some(profile), None, "default", 0, &Resources::default(), 0,
            );
            if actions.is_empty() {
                return Err("should have consolidation actions".into());
            }
            // First action should be TerminateEmpty
            if !matches!(actions[0], ConsolidationAction::TerminateEmpty(_)) {
                return Err("first action should be TerminateEmpty (empty before underutilized)".into());
            }
            Ok(())
        }),
    }
}

/// Both versions: provisioning selects the cheapest instance type that fits.
fn cross_version_provisioning_cheapest_fit_spec() -> BehaviorSpec {
    use crate::nodepool::{NodePool, NodePoolUsage};
    use crate::provisioner::provision_versioned;
    use kubesim_core::*;

    BehaviorSpec {
        name: "cross-version-provisioning-cheapest-fit",
        description: "Both versions select the cheapest fitting instance type",
        applies_to: VersionRange::all(),
        test: Box::new(|profile| {
            let mut state = ClusterState::new();
            state.submit_pod(mk_pod(1000, 1_000_000_000));

            let catalog = kubesim_ec2::Catalog::embedded()
                .map_err(|e| format!("catalog: {e}"))?;
            let pool = NodePool {
                name: "default".into(),
                instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
                ..mk_pool()
            };
            let usage = NodePoolUsage::default();

            let decisions = provision_versioned(&state, &catalog, &pool, &usage, Some(profile), &Resources::default(), 0);
            if decisions.is_empty() {
                return Err("should provision at least one node".into());
            }
            // m5.xlarge is cheaper than m5.2xlarge and fits 1000m CPU
            if decisions[0].instance_type != "m5.xlarge" {
                return Err(format!(
                    "should pick cheapest fit (m5.xlarge), got {}",
                    decisions[0].instance_type
                ));
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

fn mk_node_pool(cpu: u64, mem: u64, pool: &str) -> kubesim_core::Node {
    kubesim_core::Node {
        pool_name: pool.into(),
        ..mk_node(cpu, mem)
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

fn mk_pool() -> crate::nodepool::NodePool {
    crate::nodepool::NodePool {
        name: "default".into(),
        instance_types: vec![],
        limits: crate::nodepool::NodePoolLimits::default(),
        labels: vec![],
        taints: vec![],
        max_disrupted_pct: 10,
        max_disrupted_count: None,
        weight: 0,
        do_not_disrupt: false,
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
