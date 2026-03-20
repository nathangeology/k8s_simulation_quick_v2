# ConsolidateWhen Tradeoff Analysis

**Scenario:** `scenarios/consolidate-when/benchmark-tradeoff.yaml`
**Base workload:** benchmark-control (1→500→10 replicas scale-out/scale-in)

## Overview

This analysis systematically evaluates the `ConsolidateWhen` policy tradeoffs
using the benchmark-control workload pattern. The scale-out (1→500) followed by
scale-in (500→350→10) creates significant consolidation opportunities during the
ramp-down phases, making it ideal for measuring policy differences.

## Variant Matrix

| # | Variant | Policy | Threshold | Expected Behavior |
|---|---------|--------|-----------|-------------------|
| 1 | when-empty | WhenEmpty | — | Most conservative: only reclaims fully empty nodes |
| 2 | when-underutilized | WhenEmptyOrUnderutilized | — | Current default: consolidates empty + underutilized |
| 3 | cost-justified-0.25 | WhenCostJustifiesDisruption | 0.25 | Very aggressive: accepts small savings |
| 4 | cost-justified-0.50 | WhenCostJustifiesDisruption | 0.50 | Aggressive |
| 5 | cost-justified-0.75 | WhenCostJustifiesDisruption | 0.75 | Moderately aggressive |
| 6 | cost-justified-1.00 | WhenCostJustifiesDisruption | 1.00 | Break-even: savings ≥ disruption cost |
| 7 | cost-justified-1.50 | WhenCostJustifiesDisruption | 1.50 | Conservative: requires 50% surplus |
| 8 | cost-justified-2.00 | WhenCostJustifiesDisruption | 2.00 | Cautious: requires 2× savings |
| 9 | cost-justified-3.00 | WhenCostJustifiesDisruption | 3.00 | Very conservative |
| 10 | cost-justified-5.00 | WhenCostJustifiesDisruption | 5.00 | Minimal disruption |

## Metrics Collected

- **total_cost_per_hour** / **cumulative_cost** — cost efficiency
- **disruption_count** / **pods_evicted** — disruption impact
- **node_count** / **time_weighted_node_count** — consolidation aggressiveness
- **peak_node_count** — scale-out behavior (should be identical across variants)
- **running_pods** / **pending_pods** — workload health

## Expected Plots

### a. Cost Savings vs Threshold
X = decision_ratio_threshold, Y = cost savings (%) relative to WhenEmpty baseline.
Expected: diminishing returns curve — aggressive thresholds save more but with
decreasing marginal benefit.

### b. Disruption vs Threshold
X = threshold, Y = mean disruption count.
Expected: monotonically decreasing — higher thresholds mean fewer evictions.

### c. Cost-Disruption Pareto Frontier
X = disruption count, Y = cost savings (%). Each point is one variant.
Expected: convex frontier with a "knee" indicating the optimal tradeoff region.

### d. Node Count Over Time
One line per variant showing consolidation speed after scale-down events.
Expected: aggressive variants reclaim nodes faster; conservative variants
leave nodes running longer.

### e. Efficiency Frontier
X = threshold, Y = cost_savings_per_disruption_event.
Expected: peak at some intermediate threshold — the "sweet spot" where each
disruption event yields maximum cost benefit.

## Running the Analysis

```bash
# Run the scenario (100 seeds × 10 variants)
python -m kubesim report scenarios/consolidate-when/benchmark-tradeoff.yaml \
  --seeds 100 --output-dir results/consolidate-when/benchmark-tradeoff

# Generate tradeoff plots
python scripts/plot_consolidate_tradeoff.py \
  --results-dir results/consolidate-when/benchmark-tradeoff
```

## Findings

*To be populated after running the analysis.*

### Key Questions to Answer

1. **Where is the knee in the Pareto frontier?** — What threshold gives the best
   cost-disruption tradeoff?
2. **Does WhenCostJustifiesDisruption at t=1.0 match WhenEmptyOrUnderutilized?** —
   Are they equivalent, or does the ratio-based approach behave differently?
3. **How fast do nodes consolidate?** — Is there a meaningful difference in
   time-to-consolidate between t=0.25 and t=5.0?
4. **Is there a threshold above which savings plateau?** — Diminishing returns
   curve shape matters for default selection.
5. **What is the sweet spot?** — The efficiency frontier peak tells us the
   optimal default threshold recommendation.
