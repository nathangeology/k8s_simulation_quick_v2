# A/B Comparison Report: 3gqeqd

Variants: least_allocated, most_allocated  
Runs per variant: 5

## Variant: least_allocated

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 13324.7255 | 13324.7335 | 13324.7468 | 13324.7468 |
| Cumulative vCPU-Hours | 130603.9761 | 129947.1411 | 147805.3772 | 147805.3772 |
| Cumulative GiB-Hours | 284065.6504 | 273942.8312 | 341341.7273 | 341341.7273 |
| Time-Weighted Node Count | 299993820.0000 | 299994000.0000 | 299994300.0000 | 299994300.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 77998350.0000 | 59998800.0000 | 119997360.0000 | 119997360.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| Peak Cost Rate ($/hr) | 1.5990 | 1.5990 | 1.5990 | 1.5990 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 1.5990 | 1.5990 | 1.5990 | 1.5990 |
| Node Count | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| Running Pods | 14.4000 | 15.0000 | 16.0000 | 16.0000 |
| Pending Pods | 2.6000 | 2.0000 | 4.0000 | 4.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 29999442000000000.0000 | 29999460000000000.0000 | 29999490000000000.0000 | 29999490000000000.0000 |

## Variant: most_allocated

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 13324.7308 | 13324.7335 | 13324.7468 | 13324.7468 |
| Cumulative vCPU-Hours | 142003.7871 | 147805.3772 | 158671.6678 | 158671.6678 |
| Cumulative GiB-Hours | 290732.2731 | 285821.8354 | 341341.7273 | 341341.7273 |
| Time-Weighted Node Count | 299993940.0000 | 299994000.0000 | 299994300.0000 | 299994300.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 65998650.0000 | 59998800.0000 | 89998110.0000 | 89998110.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| Peak Cost Rate ($/hr) | 1.5990 | 1.5990 | 1.5990 | 1.5990 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 1.5990 | 1.5990 | 1.5990 | 1.5990 |
| Node Count | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| Running Pods | 14.8000 | 15.0000 | 16.0000 | 16.0000 |
| Pending Pods | 2.2000 | 2.0000 | 3.0000 | 3.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 29999454000000000.0000 | 29999460000000000.0000 | 29999490000000000.0000 | 29999490000000000.0000 |

## Comparison (Cumulative Metrics)

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| Cumulative Cost ($) | least_allocated | -0.0053 | -0.00% | 0.7441 | [-0.0213, 0.0107] |
| Cumulative vCPU-Hours | least_allocated | -11399.8110 | -8.03% | 0.2463 | [-28048.0639, 7169.7078] |
| Cumulative GiB-Hours | least_allocated | -6666.6227 | -2.29% | 0.7518 | [-60154.9308, 47063.4627] |
| Time-Weighted Node Count | least_allocated | -120.0000 | -0.00% | 0.7441 | [-480.0000, 240.0000] |
| Time to Stable (s) | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Pending Pod-Seconds | most_allocated | 11999700.0000 | 18.18% | 0.7441 | [-23999484.0000, 47998884.0000] |
| Disruption Count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Disruption-Seconds | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Peak Node Count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Peak Cost Rate ($/hr) | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |

## Comparison (End-State Diagnostics)

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| Cost Rate ($/hr) | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Node Count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Running Pods | least_allocated | -0.4000 | -2.70% | 0.7441 | [-1.6000, 0.8000] |
| Pending Pods | most_allocated | 0.4000 | 18.18% | 0.7441 | [-0.8000, 1.6000] |
| Events Processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Final Time | least_allocated | -12000000000.0000 | -0.00% | 0.7441 | [-48000000000.0000, 24000000000.0000] |
