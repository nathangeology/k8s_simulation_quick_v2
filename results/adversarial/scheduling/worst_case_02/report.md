# A/B Comparison Report: worst_case_02

Variants: least_allocated, most_allocated  
Runs per variant: 50

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 20.1342 | 20.7990 | 25.5990 | 27.1350 |
| node_count | 54.7800 | 56.5000 | 69.0000 | 73.0000 |
| running_pods | 125.2000 | 132.5000 | 141.0000 | 145.0000 |
| pending_pods | 31.1600 | 24.0000 | 61.0000 | 74.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1495037400000000.2500 | 1495140000000001.0000 | 1495560000000001.0000 | 1495740000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 20.2418 | 20.7990 | 25.5990 | 27.1350 |
| node_count | 55.0600 | 56.5000 | 69.0000 | 73.0000 |
| running_pods | 124.5000 | 131.0000 | 141.0000 | 145.0000 |
| pending_pods | 31.8800 | 25.5000 | 65.0000 | 74.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1495048800000000.2500 | 1495125000000001.0000 | 1495560000000001.0000 | 1495740000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | least_allocated | -0.1075 | -0.53% | 0.9093 | [-1.9201, 1.6316] |
| node_count | least_allocated | -0.2800 | -0.51% | 0.9093 | [-5.0005, 4.2600] |
| running_pods | most_allocated | 0.7000 | 0.56% | 0.769 | [-6.3005, 7.4600] |
| pending_pods | least_allocated | -0.7200 | -2.26% | 0.7559 | [-7.3200, 6.1200] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | least_allocated | -11400000000.0000 | -0.00% | 0.9917 | [-201000000000.0000, 175200000000.0000] |
