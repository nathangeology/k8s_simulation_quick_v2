# Test Coverage Report

Generated: 2026-03-11

## Summary

| Crate | Tests Before | Tests After | Key Areas Covered |
|-------|-------------|-------------|-------------------|
| kubesim-core | 2 | 9 | Arena + ClusterState ops (bind, evict, remove, available resources) |
| kubesim-engine | 8 | 8 | Event ordering, time modes, handler follow-ups (already well-covered) |
| kubesim-scheduler | 8 | 8 | Filter/score plugins, preemption, priority (already well-covered) |
| kubesim-karpenter | 0 | 31 | Provisioner, consolidation, spot, drift, nodepool |
| kubesim-metrics | 0 | 10 | Collector snapshots, disruption counting, export formats, percentiles, config |
| kubesim-ec2 | 0 | 0 | Catalog loads from embedded JSON (tested indirectly via karpenter) |
| kubesim-workload | 0 | 0 | Scenario/trace loading (no unit-testable pure functions without file I/O) |
| kubesim-py | 0 | 0 | PyO3 bindings (requires Python runtime) |
| **Total** | **18** | **66** | |

## Coverage by Component

### kubesim-core (9 tests)
- **Arena**: insert/get/remove, generational safety
- **ClusterState**: submit_pod, bind_pod, evict_pod, remove_pod, available_resources, error paths (bind to removed node, evict pending pod)

### kubesim-engine (8 tests)
- Empty engine returns false
- Events processed in time order
- run_until stops at boundary
- run_to_completion drains queue
- Handler receives events and schedules follow-ups
- schedule_relative: Logical ignores delay, WallClock uses delay

### kubesim-scheduler (8 tests)
- Bind pod to feasible node
- Reject when no capacity
- MostAllocated prefers fuller node
- LeastAllocated prefers emptier node
- Taint toleration (reject untolerated, allow tolerated)
- Priority ordering (high priority scheduled first)
- Not-ready nodes skipped

### kubesim-karpenter (31 tests)

**Provisioner (6 tests)**:
- Batch pending pods groups by constraints
- Separate batches for different node affinities
- Select cheapest fitting instance type
- Respect NodePool limits (max_nodes)
- Provision returns decisions for pending pods
- Empty queue returns empty decisions

**Consolidation (7 tests)**:
- WhenEmpty terminates empty nodes
- WhenEmpty skips nodes with pods
- WhenUnderutilized drains reschedulable nodes
- Disruption budget limits actions
- Disruption budget calculation (percentage, min 1)
- ConsolidationHandler schedules follow-up events
- Handler ignores non-consolidation events

**Spot Interruption (6 tests)**:
- Zero probability never interrupts
- 100% probability always interrupts
- Spot interruption evicts pods back to pending
- NodeTerminated removes node from state
- High-prob spot generates interruption + termination events
- On-demand nodes not affected by spot handler

**Drift Detection (7 tests)**:
- Drifted node detected (instance type not in pool)
- Non-drifted node passes
- Empty pool means no drift possible
- Drift handler terminates empty drifted node
- Drift handler drains node with pods
- Drift handler respects PDB (pod not evicted when PDB blocks)
- Handler ignores non-consolidation events

**NodePool (5 tests)**:
- can_launch with no limits
- can_launch blocked by node count limit
- can_launch blocked by CPU limit
- can_launch blocked by memory limit
- can_launch within all limits

### kubesim-metrics (10 tests)

**Collector (4 tests)**:
- Snapshot captures node count, cost, time
- Disruption count incremented on SpotInterruption
- CSV export has correct header
- JSON export is valid and contains expected fields

**Config (3 tests)**:
- Auto detail level resolves by pod count thresholds
- Explicit detail level unchanged by resolve
- Default config values

**Snapshot (3 tests)**:
- Percentiles from empty slice
- Percentiles from single value
- Percentiles from 100 values (p50/p90/p99 accuracy)

## Interaction Coverage

The tests verify key component interactions:
- **Karpenter triggers provisioning when pods are pending**: `provision_returns_decisions_for_pending_pods`
- **Consolidation removes underutilized nodes**: `when_underutilized_drains_reschedulable_nodes`
- **Spot interruption evicts pods**: `spot_interruption_evicts_pods`, `high_prob_spot_generates_interruptions`
- **Drift detection respects PDBs**: `drift_handler_respects_pdb`
- **Engine processes handler follow-ups**: `handler_follow_up_events_are_scheduled`

## Remaining Gaps

- **kubesim-ec2**: Catalog tested indirectly. Direct filter/query tests could be added.
- **kubesim-workload**: Scenario loading, trace replay, random generation require file fixtures or refactoring for testability.
- **kubesim-py**: PyO3 bindings require Python runtime; integration tests recommended.
- **kubesim-scheduler**: InterPodAffinity filter/scorer, PodTopologySpread filter/scorer, preemption with PDB violations not directly tested (complex setup).
- **End-to-end**: Full simulation loop (engine + scheduler + karpenter + metrics) integration test would validate the complete pipeline.
