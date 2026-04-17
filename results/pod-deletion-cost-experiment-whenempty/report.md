# Pod Deletion Cost Controller — WhenEmpty Experiment Report

Scenario: benchmark-control (500→350→10, 2 deployments)
Seeds: 10, Consolidation: WhenEmpty

## Summary Metrics

| Metric | baseline_none | pod_deletion_cost_controller | Δ (%) |
|--------|--------------|------------------------------|-------|
| disruption_count | 0.00 | 0.00 | +0.0% |
| cumulative_cost | 16.78 | 16.17 | -3.6% |
| time_weighted_node_count | 10200.00 | 8580.00 | -15.9% |
| node_count | 3.00 | 1.00 | -66.7% |
| time_to_stable | 1680.00 | 1860.00 | +10.7% |

## Per-Seed Detail

| Seed | Variant | disruption_count | cumulative_cost | time_weighted_node_count | final_nodes |
|------|---------|-----------------|-----------------|-------------------------|-------------|
| 0 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 0 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 1 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 1 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 2 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 2 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 3 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 3 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 4 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 4 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 5 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 5 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 6 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 6 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 7 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 7 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 8 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 8 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
| 9 | baseline_none | 0 | 16.78 | 10200.0 | 3 |
| 9 | pod_deletion_cost_controller | 0 | 16.17 | 8580.0 | 1 |
