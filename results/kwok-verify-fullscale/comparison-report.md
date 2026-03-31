# Full-Scale KWOK Verification Report

Generated: 2026-03-31T07:39:58Z
Parameters: 500 replicas, 35-min sequence, consolidateAfter=30s, 60s metrics

## Acceptance Criteria

| Criterion | Pass | Detail |
|-----------|------|--------|
| WhenEmpty zero-disruption | ❌ | evictions=32 |
| Disruption ordering: wu > cj-1.00 > cj-5.00 ≥ we | ❌ | 36 > 49 > 16 ≥ 32 |

## Per-Variant Results

| Variant | KWOK Evictions | Sim Disruptions | KWOK Final Nodes |
|---------|---------------|-----------------|------------------|
| when-empty | 32 | 0.0 | 20 |
| when-underutilized | 36 | 438.25 | 7 |
| cost-justified-1.00 | 49 | 54.3 | 10 |
| cost-justified-5.00 | 16 | 1.95 | 20 |

## Simulator Predictions (reference)

From `results/consolidate-when/benchmark-tradeoff-kwok/results.json`:
- WhenEmpty: 0.0 disruptions (baseline)
- WhenEmptyOrUnderutilized: 438.2 disruptions (most aggressive)
- CostJustified-1.00: 54.3 disruptions (knee point)
- CostJustified-5.00: 1.95 disruptions (conservative)

Expected ordering: wu(438) >> cj-1.00(54) >> cj-5.00(2) >> we(0)

## Node Count Gradient

If the KWOK results show differentiated final node counts across variants,
the cost-justified threshold is controlling consolidation aggressiveness
as designed. Lower thresholds → more consolidation → fewer final nodes.
