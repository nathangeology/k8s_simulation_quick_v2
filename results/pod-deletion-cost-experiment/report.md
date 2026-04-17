# Pod Deletion Cost Controller — Experiment Report

Scenario: benchmark-control (500→350→10, 2 deployments)
Seeds: 10, Consolidation: WhenUnderutilized

## Summary Metrics

| Metric | baseline_none | pod_deletion_cost_controller | Δ (%) |
|--------|--------------|------------------------------|-------|
| disruption_count | 266.90 | 346.00 | +29.6% |
| cumulative_cost | 13.11 | 13.43 | +2.4% |
| time_weighted_node_count | 6780.00 | 6930.00 | +2.2% |
| node_count | 1.00 | 1.00 | +0.0% |
| time_to_stable | 1740.00 | 1800.00 | +3.4% |

## Per-Seed Detail

| Seed | Variant | disruption_count | cumulative_cost | time_weighted_node_count | final_nodes |
|------|---------|-----------------|-----------------|-------------------------|-------------|
| 0 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 0 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 1 | baseline_none | 266 | 13.11 | 6780.0 | 1 |
| 1 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 2 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 2 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 3 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 3 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 4 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 4 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 5 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 5 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 6 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 6 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 7 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 7 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 8 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 8 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |
| 9 | baseline_none | 267 | 13.11 | 6780.0 | 1 |
| 9 | pod_deletion_cost_controller | 346 | 13.43 | 6930.0 | 1 |