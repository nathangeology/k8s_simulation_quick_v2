# A/B Comparison Report: worst_case_11

Variants: least_allocated, most_allocated  
Runs per variant: 50

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 0.0960 | 0.0960 | 0.0960 | 0.0960 |
| node_count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| running_pods | 4.0000 | 4.0000 | 4.0000 | 4.0000 |
| pending_pods | 3.0000 | 3.0000 | 3.0000 | 3.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1499850000000000.2500 | 1499850000000001.0000 | 1499850000000001.0000 | 1499850000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 0.0960 | 0.0960 | 0.0960 | 0.0960 |
| node_count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| running_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| pending_pods | 7.0000 | 7.0000 | 7.0000 | 7.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1499850000000000.2500 | 1499850000000001.0000 | 1499850000000001.0000 | 1499850000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| node_count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| running_pods | most_allocated | 4.0000 | 400.00% | 0 | [4.0000, 4.0000] |
| pending_pods | least_allocated | -4.0000 | -57.14% | 0 | [-4.0000, -4.0000] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
