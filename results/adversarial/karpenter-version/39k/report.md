# A/B Comparison Report: 39k

Variants: karpenter-v0.35, karpenter-v1.x  
Runs per variant: 50

## Variant: karpenter-v0.35

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.4054 | 3.8290 | 3.9170 | 3.9170 |
| node_count | 14.3400 | 16.0000 | 16.0000 | 16.0000 |
| running_pods | 113.3600 | 122.0000 | 125.0000 | 127.0000 |
| pending_pods | 65.6400 | 57.0000 | 85.0000 | 85.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 |

## Variant: karpenter-v1.x

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.2014 | 3.5290 | 3.6770 | 3.6770 |
| node_count | 14.3400 | 16.0000 | 16.0000 | 16.0000 |
| running_pods | 112.1400 | 116.0000 | 128.0000 | 128.0000 |
| pending_pods | 66.8600 | 63.0000 | 89.0000 | 90.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 | 420000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | karpenter-v1.x | 0.2040 | 6.37% | 2e-06 | [-0.0380, 0.4305] |
| node_count | tie | 0.0000 | 0.00% | 1 | [-0.9205, 0.8800] |
| running_pods | karpenter-v1.x | 1.2200 | 1.09% | 0.8833 | [-4.8000, 6.9205] |
| pending_pods | karpenter-v0.35 | -1.2200 | -1.82% | 0.8833 | [-6.9205, 4.8000] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
