# A/B Comparison Report: worst_case_01

Variants: least_allocated, most_allocated  
Runs per variant: 50

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 19.7699 | 19.4550 | 25.5990 | 27.1350 |
| node_count | 53.8200 | 53.0000 | 69.0000 | 73.0000 |
| running_pods | 117.2200 | 118.5000 | 149.0000 | 152.0000 |
| pending_pods | 35.8600 | 34.5000 | 70.0000 | 81.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1494912000000000.2500 | 1494945000000001.0000 | 1495560000000001.0000 | 1495740000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 18.5257 | 17.5350 | 25.5990 | 27.1350 |
| node_count | 50.5800 | 48.0000 | 69.0000 | 73.0000 |
| running_pods | 111.1200 | 108.5000 | 145.0000 | 151.0000 |
| pending_pods | 41.7600 | 45.5000 | 72.0000 | 82.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1494765600000000.2500 | 1494720000000001.0000 | 1495560000000001.0000 | 1495740000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | most_allocated | 1.2442 | 6.72% | 0.2167 | [-0.7910, 3.2870] |
| node_count | most_allocated | 3.2400 | 6.41% | 0.2167 | [-2.0600, 8.5600] |
| running_pods | most_allocated | 6.1000 | 5.49% | 0.2008 | [-3.3405, 15.2800] |
| pending_pods | least_allocated | -5.9000 | -14.13% | 0.196 | [-14.8600, 3.3010] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | most_allocated | 146400000000.0000 | 0.01% | 0.1912 | [-85214999999.9999, 373800000000.0000] |
