# A/B Comparison Report: most-vs-least-allocated

Variants: least_allocated, most_allocated  
Runs per variant: 100

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 1.3819 | 1.3220 | 1.7060 | 2.1120 |
| node_count | 4.1900 | 4.0000 | 5.0000 | 6.0000 |
| running_pods | 42.0400 | 41.5000 | 58.0000 | 68.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 2998397400000000.5000 | 2998425000000001.0000 | 2998950000000001.0000 | 2999130000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 1.3244 | 1.3220 | 1.7060 | 1.7060 |
| node_count | 4.0500 | 4.0000 | 5.0000 | 5.0000 |
| running_pods | 42.0400 | 41.5000 | 58.0000 | 68.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 2998434300000000.5000 | 2998485000000001.0000 | 2998980000000001.0000 | 2999160000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | most_allocated | 0.0575 | 4.34% | 0.2491 | [-0.0342, 0.1491] |
| node_count | most_allocated | 0.1400 | 3.46% | 0.265 | [-0.0902, 0.3700] |
| running_pods | tie | 0.0000 | 0.00% | 1 | [-3.4402, 3.4800] |
| pending_pods | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | least_allocated | -36900000000.0000 | -0.00% | 0.503 | [-153000000000.0000, 79200000000.0000] |
