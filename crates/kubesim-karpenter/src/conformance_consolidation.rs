//! Conformance specs for consolidation behaviors.

#[cfg(test)]
mod tests {
    use crate::conformance::{BehaviorSpec, run_specs, SpecResult};
    use crate::consolidation::{evaluate_versioned, ConsolidationAction, ConsolidationPolicy};
    use crate::version::{KarpenterVersion, VersionProfile};
    use kubesim_core::*;

    fn node(cpu: u64, mem: u64, pool: &str) -> Node {
        Node {
            instance_type: "m5.xlarge".into(),
            allocatable: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            allocated: Resources::default(),
            pods: smallvec::smallvec![],
            conditions: NodeConditions { ready: true, ..Default::default() },
            labels: LabelSet::default(),
            taints: smallvec::smallvec![],
            cost_per_hour: 0.192,
            lifecycle: NodeLifecycle::OnDemand,
            cordoned: false,
            created_at: SimTime(0),
            pool_name: pool.into(),
            do_not_disrupt: false,
        }
    }

    fn pod(cpu: u64, mem: u64) -> Pod {
        Pod {
            requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            limits: Resources::default(),
            phase: PodPhase::Pending,
            node: None,
            scheduling_constraints: SchedulingConstraints::default(),
            deletion_cost: None,
            owner: OwnerId(0),
            qos_class: QoSClass::Burstable,
            priority: 0,
            labels: LabelSet::default(),
            do_not_disrupt: false,
            duration_ns: None,
        }
    }

    /// Spec 1: candidate_score sorts by pod count (lower = better candidate),
    /// not by node capacity. Given two underutilized nodes with different pod
    /// counts but same capacity, the one with fewer pods is consolidated first.
    fn prefer_emptying_nodes_sorts_by_pod_count() -> Result<(), String> {
        let mut state = ClusterState::new();

        // Node A: 1 pod (should be preferred for consolidation)
        let na = state.add_node(node(4000, 8_000_000_000, "default"));
        let p1 = state.submit_pod(pod(500, 500_000_000));
        state.bind_pod(p1, na);

        // Node B: 3 pods (higher pod count = worse candidate)
        let nb = state.add_node(node(4000, 8_000_000_000, "default"));
        for _ in 0..3 {
            let p = state.submit_pod(pod(200, 200_000_000));
            state.bind_pod(p, nb);
        }

        // Node C: absorber with plenty of capacity
        state.add_node(node(8000, 16_000_000_000, "default"));

        let profile = VersionProfile::new(KarpenterVersion::V1);
        let actions = evaluate_versioned(
            &state, ConsolidationPolicy::WhenUnderutilized, 10,
            Some(&profile), None, "default", 0,
        );

        // Both should be candidates, but node A (1 pod) should appear first
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
    }

    /// Spec 2: Empty-node consolidation fires before underutilized-node consolidation.
    /// When both empty and underutilized nodes exist, TerminateEmpty actions
    /// appear before DrainAndTerminate actions in the result.
    fn empty_before_underutilized() -> Result<(), String> {
        let mut state = ClusterState::new();

        // Empty node
        state.add_node(node(4000, 8_000_000_000, "default"));

        // Underutilized node with a small pod
        let nu = state.add_node(node(4000, 8_000_000_000, "default"));
        let p = state.submit_pod(pod(500, 500_000_000));
        state.bind_pod(p, nu);

        // Absorber
        state.add_node(node(8000, 16_000_000_000, "default"));

        let profile = VersionProfile::new(KarpenterVersion::V1);
        let actions = evaluate_versioned(
            &state, ConsolidationPolicy::WhenUnderutilized, 10,
            Some(&profile), None, "default", 0,
        );

        if actions.is_empty() {
            return Err("expected consolidation actions".into());
        }

        // Find the index of first TerminateEmpty and first DrainAndTerminate
        let first_empty = actions.iter().position(|a| matches!(a, ConsolidationAction::TerminateEmpty(_)));
        let first_drain = actions.iter().position(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. }));

        match (first_empty, first_drain) {
            (Some(e), Some(d)) if e < d => Ok(()),
            (Some(_), None) => Ok(()), // only empty actions is fine
            (None, Some(_)) => Err("empty node should have been terminated before drain".into()),
            (Some(e), Some(d)) => Err(format!("empty at index {} but drain at index {}", e, d)),
            (None, None) => Err("no consolidation actions produced".into()),
        }
    }

    /// Spec 3: Greedy candidate selection excludes nodes already selected for removal.
    /// When checking if a candidate's pods can reschedule, nodes already picked
    /// for removal are excluded from available capacity.
    fn greedy_excludes_already_selected() -> Result<(), String> {
        let mut state = ClusterState::new();

        // Node A: 1 small pod — will be first candidate (fewer pods)
        let na = state.add_node(node(4000, 8_000_000_000, "default"));
        let pa = state.submit_pod(pod(500, 500_000_000));
        state.bind_pod(pa, na);

        // Node B: 1 small pod — second candidate
        let nb = state.add_node(node(4000, 8_000_000_000, "default"));
        let pb = state.submit_pod(pod(500, 500_000_000));
        state.bind_pod(pb, nb);

        // Node C: absorber, but only enough spare capacity for ONE extra pod
        let nc = state.add_node(node(4000, 8_000_000_000, "default"));
        let pc = state.submit_pod(pod(3000, 7_000_000_000));
        state.bind_pod(pc, nc);

        let profile = VersionProfile::new(KarpenterVersion::V1);
        let actions = evaluate_versioned(
            &state, ConsolidationPolicy::WhenUnderutilized, 10,
            Some(&profile), None, "default", 0,
        );

        // Only ONE drain should succeed: after the first candidate is selected,
        // the absorber's remaining capacity can't fit the second candidate's pod.
        let drain_count = actions.iter().filter(|a| matches!(a, ConsolidationAction::DrainAndTerminate { .. })).count();
        if drain_count > 1 {
            return Err(format!(
                "expected at most 1 drain (greedy exclusion), got {}",
                drain_count
            ));
        }
        if drain_count == 0 {
            return Err("expected 1 drain action but got 0".into());
        }
        Ok(())
    }

    /// Spec 4: ConsolidateAfter exempts recently-created nodes from consolidation.
    /// Nodes younger than consolidate_after_ns are not considered as candidates.
    fn consolidate_after_exempts_young_nodes() -> Result<(), String> {
        let mut state = ClusterState::new();
        state.time = SimTime(100_000_000_000); // 100s into simulation

        // Young empty node (created at t=90s, age=10s)
        let mut young = node(4000, 8_000_000_000, "default");
        young.created_at = SimTime(90_000_000_000);
        state.add_node(young);

        // Old empty node (created at t=0, age=100s)
        state.add_node(node(4000, 8_000_000_000, "default"));

        let consolidate_after = 30_000_000_000u64; // 30s
        let profile = VersionProfile::new(KarpenterVersion::V1);
        let actions = evaluate_versioned(
            &state, ConsolidationPolicy::WhenEmpty, 10,
            Some(&profile), None, "default", consolidate_after,
        );

        // Only the old node should be terminated; young node is exempt
        if actions.len() != 1 {
            return Err(format!("expected 1 action (old node only), got {}", actions.len()));
        }
        Ok(())
    }

    fn consolidation_specs() -> Vec<BehaviorSpec> {
        vec![
            BehaviorSpec {
                name: "prefer-emptying-nodes-sorts-by-pod-count",
                description: "PreferEmptyingNodes sorts candidates by pod count, not node capacity",
                applies_to: &[KarpenterVersion::V0_35, KarpenterVersion::V1],
                test_fn: prefer_emptying_nodes_sorts_by_pod_count,
            },
            BehaviorSpec {
                name: "empty-node-before-underutilized",
                description: "Empty-node consolidation fires before underutilized-node consolidation",
                applies_to: &[KarpenterVersion::V0_35, KarpenterVersion::V1],
                test_fn: empty_before_underutilized,
            },
            BehaviorSpec {
                name: "greedy-excludes-already-selected",
                description: "Greedy candidate selection excludes nodes already selected for removal",
                applies_to: &[KarpenterVersion::V1],
                test_fn: greedy_excludes_already_selected,
            },
            BehaviorSpec {
                name: "consolidate-after-exempts-young-nodes",
                description: "ConsolidateAfter exempts recently-created nodes from consolidation",
                applies_to: &[KarpenterVersion::V0_35, KarpenterVersion::V1],
                test_fn: consolidate_after_exempts_young_nodes,
            },
        ]
    }

    #[test]
    fn conformance_v1() {
        let specs = consolidation_specs();
        let results = run_specs(&specs, KarpenterVersion::V1);
        for (name, result) in &results {
            match result {
                SpecResult::Pass => {}
                SpecResult::Skipped(reason) => {
                    println!("SKIP {name}: {reason}");
                }
                SpecResult::Fail(reason) => {
                    panic!("FAIL {name}: {reason}");
                }
            }
        }
    }

    #[test]
    fn conformance_v0_35() {
        let specs = consolidation_specs();
        let results = run_specs(&specs, KarpenterVersion::V0_35);
        for (name, result) in &results {
            match result {
                SpecResult::Pass => {}
                SpecResult::Skipped(reason) => {
                    println!("SKIP {name}: {reason}");
                }
                SpecResult::Fail(reason) => {
                    panic!("FAIL {name}: {reason}");
                }
            }
        }
    }
}
