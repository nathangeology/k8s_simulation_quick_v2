# A/B Comparison Report: most-vs-least-allocated

Variants: least_allocated, most_allocated  
Runs per variant: 100

## Variant: least_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 0.5780 | 0.6800 | 0.6800 | 1.0200 |
| node_count | 1.8300 | 2.0000 | 2.0000 | 3.0000 |
| running_pods | 34.0800 | 36.5000 | 49.0000 | 55.0000 |
| pending_pods | 177.7700 | 176.5000 | 257.0000 | 302.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1496724600000000.2500 | 1496730000000001.0000 | 1497870000000001.0000 | 1498410000000001.0000 |

## Variant: most_allocated

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 0.5644 | 0.6800 | 0.6800 | 0.6800 |
| node_count | 1.6900 | 2.0000 | 2.0000 | 2.0000 |
| running_pods | 32.9200 | 31.5000 | 55.0000 | 64.0000 |
| pending_pods | 178.9300 | 170.0000 | 258.0000 | 295.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1496733900000000.2500 | 1496730000000001.0000 | 1497870000000001.0000 | 1498410000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | most_allocated | 0.0136 | 2.41% | 0.9571 | [-0.0340, 0.0629] |
| node_count | most_allocated | 0.1400 | 8.28% | 0.03185 | [0.0200, 0.2600] |
| running_pods | most_allocated | 1.1600 | 3.52% | 0.4383 | [-2.8900, 5.1703] |
| pending_pods | least_allocated | -1.1600 | -0.65% | 0.8632 | [-17.7102, 15.5605] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | least_allocated | -9300000000.0000 | -0.00% | 0.9192 | [-256522499999.9999, 234007499999.9999] |
