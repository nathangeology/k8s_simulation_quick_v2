# A/B Comparison Report: karpenter-version-comparison

Variants: karpenter-v0.35, karpenter-v1.x  
Runs per variant: 100

## Variant: karpenter-v0.35

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 1.1520 | 1.1520 | 1.1520 | 1.1520 |
| node_count | 2.0000 | 2.0000 | 2.0000 | 2.0000 |
| running_pods | 50.7600 | 52.0000 | 52.0000 | 52.0000 |
| pending_pods | 1.2400 | 0.0000 | 4.0000 | 6.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 2997918900000000.5000 | 2997930000000001.0000 | 2997930000000001.0000 | 2997930000000001.0000 |

## Variant: karpenter-v1.x

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 1.2303 | 1.1520 | 1.3220 | 2.0900 |
| node_count | 2.3900 | 2.0000 | 3.0000 | 4.0000 |
| running_pods | 52.0000 | 52.0000 | 52.0000 | 52.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 2997906600000000.5000 | 2997930000000001.0000 | 2997930000000001.0000 | 2997930000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | karpenter-v0.35 | -0.0783 | -6.36% | 0 | [-0.1098, -0.0527] |
| node_count | karpenter-v0.35 | -0.3900 | -16.32% | 0 | [-0.4900, -0.2900] |
| running_pods | karpenter-v0.35 | -1.2400 | -2.38% | 0 | [-1.6002, -0.9000] |
| pending_pods | karpenter-v1.x | 1.2400 | 124.00% | 0 | [0.9000, 1.6002] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | karpenter-v1.x | 12300000000.0000 | 0.00% | 0.05176 | [5400000000.0000, 19500000000.0000] |
