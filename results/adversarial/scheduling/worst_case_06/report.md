# A/B Comparison Report: worst_case_06

Variants: least_allocated, most_allocated  
Runs per variant: 50

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.3604 | 3.3380 | 4.1060 | 4.4050 |
| node_count | 18.7600 | 19.0000 | 23.0000 | 24.0000 |
| running_pods | 75.7600 | 78.0000 | 79.0000 | 79.0000 |
| pending_pods | 3.2400 | 1.0000 | 10.0000 | 25.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1498813800000000.2500 | 1498830000000001.0000 | 1498980000000001.0000 | 1499130000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.3374 | 3.2530 | 4.1060 | 4.4050 |
| node_count | 18.6400 | 18.0000 | 23.0000 | 24.0000 |
| running_pods | 75.5800 | 78.5000 | 79.0000 | 79.0000 |
| pending_pods | 3.4200 | 0.5000 | 14.0000 | 31.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1498807200000000.2500 | 1498830000000001.0000 | 1498980000000001.0000 | 1499070000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | most_allocated | 0.0230 | 0.69% | 0.7716 | [-0.2074, 0.2607] |
| node_count | most_allocated | 0.1200 | 0.64% | 0.7813 | [-1.0400, 1.3400] |
| running_pods | most_allocated | 0.1800 | 0.24% | 0.803 | [-2.2000, 2.6000] |
| pending_pods | least_allocated | -0.1800 | -5.26% | 0.803 | [-2.6000, 2.2000] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | most_allocated | 6600000000.0000 | 0.00% | 0.8544 | [-46800000000.0000, 58200000000.0000] |
