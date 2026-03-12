# A/B Comparison Report: worst_case_04

Variants: least_allocated, most_allocated  
Runs per variant: 50

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 22.8496 | 23.6790 | 27.1350 | 27.1350 |
| node_count | 61.8400 | 64.0000 | 73.0000 | 73.0000 |
| running_pods | 167.6400 | 171.0000 | 176.0000 | 177.0000 |
| pending_pods | 9.3600 | 6.0000 | 20.0000 | 39.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1494722400000000.2500 | 1494750000000001.0000 | 1495110000000001.0000 | 1495110000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 22.8683 | 23.8710 | 27.1350 | 27.1350 |
| node_count | 61.9000 | 64.5000 | 73.0000 | 73.0000 |
| running_pods | 167.8200 | 171.0000 | 177.0000 | 177.0000 |
| pending_pods | 9.1800 | 6.0000 | 20.0000 | 45.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1494725400000000.2500 | 1494780000000001.0000 | 1495110000000001.0000 | 1495110000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | least_allocated | -0.0188 | -0.08% | 0.9584 | [-1.3619, 1.2989] |
| node_count | least_allocated | -0.0600 | -0.10% | 0.9334 | [-3.5600, 3.3800] |
| running_pods | least_allocated | -0.1800 | -0.11% | 0.8062 | [-3.9200, 3.7600] |
| pending_pods | most_allocated | 0.1800 | 1.96% | 0.8062 | [-3.7600, 3.9200] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | least_allocated | -3000000000.0000 | -0.00% | 0.9421 | [-124814999999.9999, 116400000000.0000] |
