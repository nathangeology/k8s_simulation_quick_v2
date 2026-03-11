# A/B Comparison Report: 3gqeqd

Variants: least_allocated, most_allocated  
Runs per variant: 50

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 1.5990 | 1.5990 | 1.5990 | 1.5990 |
| node_count | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| running_pods | 13.4000 | 14.0000 | 15.0000 | 16.0000 |
| pending_pods | 2.6000 | 2.0000 | 4.0000 | 5.0000 |
| events_processed | 17.0000 | 17.0000 | 17.0000 | 17.0000 |
| final_time | 60000000000.0000 | 60000000000.0000 | 60000000000.0000 | 60000000000.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 1.5990 | 1.5990 | 1.5990 | 1.5990 |
| node_count | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| running_pods | 13.6600 | 14.0000 | 15.0000 | 16.0000 |
| pending_pods | 2.3400 | 2.0000 | 4.0000 | 5.0000 |
| events_processed | 17.0000 | 17.0000 | 17.0000 | 17.0000 |
| final_time | 60000000000.0000 | 60000000000.0000 | 60000000000.0000 | 60000000000.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| node_count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| running_pods | least_allocated | -0.2600 | -1.90% | 0.2832 | [-0.7000, 0.2000] |
| pending_pods | most_allocated | 0.2600 | 11.11% | 0.2832 | [-0.2000, 0.7000] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
