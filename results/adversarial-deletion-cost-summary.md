# Adversarial Discovery: Deletion Cost Ranking Strategies

## Overview

5-way adversarial comparison of deletion cost ranking strategies using
coverage-guided search (budget=500 normal + 500 chaos, 1000 total scenarios).

**Strategies compared:**
- `baseline` (none) — no deletion-cost annotations
- `smallest_first` (prefer_emptying_nodes) — prefer deleting from nearly-empty nodes
- `largest_first` — delete from largest-capacity nodes first
- `unallocated_vcpu` — target inefficiently packed nodes
- `random` — random deletion ordering

**Objectives:** availability, cost_efficiency, disruption_rate

**Seeds per evaluation:** 3 (42, 100, 200)

## Key Findings

### 1. No differentiation among structured strategies

All four non-baseline strategies (`smallest_first`, `largest_first`,
`unallocated_vcpu`, `random`) produce **identical results** across all
1000 scenarios tested. They cluster together perfectly on every metric:
cost, availability, node count, running pods, and pending pods.

This means the deletion cost ranking logic — while it does differ from
having no annotations at all — does not produce observable differences
between the four ranking approaches in the current simulator.

### 2. Baseline vs everything else

The only divergence found is between `baseline` (no annotations) and
the four annotation strategies. In the top scenarios:

| Scenario | Divergence | Baseline better? | Notes |
|----------|-----------|-----------------|-------|
| #1 (chaos) | 1.36 | Yes (cost_eff) | Extreme: 451 pending pods, 1 node. Baseline gets slightly more running pods |
| #2 | 0.30 | Mixed | Baseline: 70% availability, others: 100%. But baseline lower cost |
| #3 | 0.13 | Yes (avail) | Baseline: 100% availability, others: 86.7% |
| #4 | 0.056 | Mixed | Others have better availability but higher cost |
| #5 | 0.052 | Mixed | Others achieve 100% availability vs baseline 94.8% |

### 3. No disruption rate differences

Disruption rate is 0.0 across all strategies in all scenarios. The deletion
cost annotations do not affect pod eviction behavior in the tested scenarios.

### 4. Random never beats structured strategies

Random produces identical results to all structured strategies. There are
no scenarios where random ordering outperforms or underperforms the
purpose-built ranking approaches.

### 5. Baseline is unpredictable

Baseline (no annotations) sometimes outperforms (lower cost, higher
availability) and sometimes underperforms the annotation strategies.
The direction of divergence is scenario-dependent.

## Interpretation

The four deletion cost ranking strategies (`smallest_first`, `largest_first`,
`unallocated_vcpu`, `random`) all set pod-deletion-cost annotations, but the
specific ranking values don't produce different outcomes. This suggests:

1. **The annotation presence matters, not the ranking.** Having any
   deletion-cost annotation changes ReplicaSet scale-down behavior vs
   having none, but the specific cost values don't influence which pods
   Karpenter consolidates.

2. **Karpenter consolidation dominates.** Node deletion ordering during
   consolidation is driven by Karpenter's own logic (emptiness, cost),
   not by pod-deletion-cost annotations. The annotations only affect
   which pods the RS controller removes during scale-down.

3. **Scale-down events are rare in generated scenarios.** The adversarial
   search found few scenarios where RS scale-down interacts meaningfully
   with node consolidation — the two systems operate somewhat independently.

## Statistics

- Total scenarios evaluated: 1000 (500 normal, 500 chaos)
- Scenarios with divergence > 0: 70 (7%)
- Max divergence: 1.36
- Mean divergence (nonzero): 0.035

## Files

- Script: `scripts/find_adversarial_deletion_cost.py`
- Scenarios: `scenarios/adversarial/deletion-cost/`
- Reports: `results/adversarial/deletion-cost/`
- Manifest: `results/adversarial/deletion-cost/manifest.json`
