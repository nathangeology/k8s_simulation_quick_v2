# A/B Comparison Report: karpenter-version-comparison

Variants: karpenter-v0.35, karpenter-v1.x  
Runs per variant: 100

## Variant: karpenter-v0.35

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 0.7680 | 0.7680 | 0.7680 | 0.7680 |
| node_count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| running_pods | 20.5900 | 20.0000 | 23.0000 | 25.0000 |
| pending_pods | 39.8500 | 40.0000 | 42.0000 | 45.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1498205400000000.2500 | 1498230000000001.0000 | 1498230000000001.0000 | 1498230000000001.0000 |

## Variant: karpenter-v1.x

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 0.3840 | 0.3840 | 0.3840 | 0.3840 |
| node_count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| running_pods | 13.8400 | 13.0000 | 18.0000 | 31.0000 |
| pending_pods | 47.0800 | 47.0000 | 51.0000 | 52.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 1498095900000000.2500 | 1498110000000001.0000 | 1498170000000001.0000 | 1498170000000001.0000 |

## Comparison

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| total_cost_per_hour | karpenter-v1.x | 0.3840 | 100.00% | 0 | [0.3840, 0.3840] |
| node_count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| running_pods | karpenter-v1.x | 6.7500 | 48.77% | 0 | [5.8797, 7.5800] |
| pending_pods | karpenter-v0.35 | -7.2300 | -15.36% | 0 | [-7.9002, -6.5400] |
| events_processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| final_time | karpenter-v1.x | 109500000000.0000 | 0.01% | 0 | [94800000000.0000, 124200000000.0000] |
