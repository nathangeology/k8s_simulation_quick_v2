# Adversarial Scenario Results

Generated: 2026-03-11 20:35 UTC
Budget: 1000 scenarios evaluated

## Key Finding: Zero Cost Divergence

**All 1000 scenarios produced identical `total_cost_per_hour` for MostAllocated vs LeastAllocated.**

### Root Cause

The simulation engine provisions nodes deterministically from the scenario's `node_pools`
configuration at load time via `NodeLaunching` events. The scoring strategy (MostAllocated
vs LeastAllocated) only affects *which* node a pod is placed on — it does not trigger
dynamic node provisioning or consolidation. Since both variants operate on the same set
of pre-provisioned nodes, `total_cost_per_hour` (sum of all node costs) is always identical.

For the scoring strategy to produce cost divergence, the engine would need to:
1. Dynamically provision new nodes when existing ones are full (Karpenter provisioning loop)
2. Consolidate underutilized nodes (Karpenter consolidation loop)

The `KarpenterProvisioningLoop` and `KarpenterConsolidationLoop` events are scheduled but
the `SimHandler` in `kubesim-py/src/lib.rs` only handles `PodSubmitted` events — provisioning
and consolidation events are no-ops in the current engine.

### Pod Placement Differences

While cost is identical, pod placement does vary. In worst_case_03 (the only scenario with
meaningful workload), MostAllocated achieved slightly more running pods (13.66 vs 13.40,
p=0.28) by bin-packing pods onto fewer nodes, leaving more capacity on remaining nodes.
This difference is not statistically significant.

## MostAllocated costs more (0 scenarios)

_No scenarios found in this category._

## LeastAllocated costs more (0 scenarios)

_No scenarios found in this category._

## Both degrade (mixed signals) (1000 scenarios)

All 1000 scenarios fell into this category with zero delta.

File                         Δ cost/hr  Most $/hr Least $/hr
--------------------------------------------------------------
worst_case_01.yaml             +0.0000     0.0960     0.0960
worst_case_02.yaml             +0.0000     0.1920     0.1920
worst_case_03.yaml             +0.0000     1.5990     1.5990
worst_case_04.yaml             +0.0000     0.3840     0.3840
worst_case_05.yaml             +0.0000     0.1920     0.1920

## A/B Report Highlights (Top 5, 50 seeds each)

| Scenario | Nodes | Most Running | Least Running | Cost Delta | p-value |
|----------|-------|-------------|---------------|------------|---------|
| worst_case_01 | 1 | 0.0 | 0.0 | 0.0000 | 1.0 |
| worst_case_02 | 1 | 0.0 | 0.0 | 0.0000 | 1.0 |
| worst_case_03 | 10 | 13.66 | 13.40 | 0.0000 | 0.28 |
| worst_case_04 | 1 | 0.0 | 0.0 | 0.0000 | 1.0 |
| worst_case_05 | 1 | 0.0 | 0.0 | 0.0000 | 1.0 |

## Recommendations

1. **Implement Karpenter provisioning/consolidation in SimHandler** — The engine schedules
   `KarpenterProvisioningLoop` and `KarpenterConsolidationLoop` events but doesn't handle
   them. Adding dynamic node scaling would make scoring strategy affect node count and cost.

2. **Generate scenarios with tighter node capacity** — Scenarios where `min_nodes` provides
   just barely enough capacity would force the strategies to diverge on pod placement
   efficiency, potentially triggering different scaling decisions once Karpenter is wired up.

3. **Re-run adversarial search after engine enhancement** — Once dynamic provisioning is
   implemented, the adversarial finder should discover meaningful divergence.
