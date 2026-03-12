# A/B Comparison Report: r7vglr

Variants: karpenter-v0.35, karpenter-v1.x  
Runs per variant: 50

## Variant: karpenter-v0.35

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 9.1740 | 8.1880 | 11.2480 | 13.2880 |
| node_count | 22.9800 | 22.0000 | 27.0000 | 32.0000 |
| running_pods | 17.9000 | 17.5000 | 23.0000 | 26.0000 |
| pending_pods | 42.1000 | 42.5000 | 47.0000 | 48.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 |

## Variant: karpenter-v1.x

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 8.5702 | 8.0400 | 10.3600 | 12.4000 |
| node_count | 22.9800 | 22.0000 | 27.0000 | 32.0000 |
| running_pods | 18.1000 | 18.0000 | 23.0000 | 26.0000 |
| pending_pods | 41.9000 | 42.0000 | 47.0000 | 48.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | karpenter-v1.x | 0.6038 | 7.05% | 0.0508 | [-0.0672, 1.3111] |
| node_count | tie | 0.0000 | 0.00% | 1 | [-1.1800, 1.2200] |
| running_pods | karpenter-v0.35 | -0.2000 | -1.10% | 0.7655 | [-1.5800, 1.1600] |
| pending_pods | karpenter-v1.x | 0.2000 | 0.48% | 0.7655 | [-1.1600, 1.5800] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
