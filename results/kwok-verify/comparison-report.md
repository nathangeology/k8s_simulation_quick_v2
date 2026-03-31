# KWOK vs Simulator Comparison Report

Generated: 2026-03-31T03:15:57.205029Z

## Structural Criteria

| Criterion | Pass | Detail |
|-----------|------|--------|
| WhenEmpty zero-disruption | ✅ | evictions=0 |
| WhenEmptyOrUnderutilized most disruptive | ❌ | wu_evictions=0, max_other=1 |
| Disruption monotonicity (higher threshold → fewer evictions) | ❌ | evictions=[1, 0, 0, 0, 1, 0, 1, 1] |

## Per-Variant Eviction Comparison

| Variant | Sim Evictions | KWOK Evictions | Δ | Within Tolerance |
|---------|--------------|----------------|---|-----------------|

## Per-Variant Node Count Comparison

| Variant | Sim Nodes | KWOK Nodes | Rel Δ | Within Tolerance |
|---------|-----------|------------|-------|-----------------|

## Verdict

Structural criteria: FAILURES DETECTED ❌

