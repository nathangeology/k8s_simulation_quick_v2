# A/B Comparison Report: alccepyoe

Variants: least_allocated, most_allocated  
Runs per variant: 5

## Variant: least_allocated

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 1958.9655 | 1546.5809 | 2577.5425 | 2577.5425 |
| Cumulative vCPU-Hours | 15478.8700 | 12220.0833 | 20367.0500 | 20367.0500 |
| Cumulative GiB-Hours | 30957.7400 | 24440.1667 | 40734.1000 | 40734.1000 |
| Time-Weighted Node Count | 27865881.0000 | 22000065.0000 | 36664605.0000 | 36664605.0000 |
| Time to Stable (s) | 480.0000 | 480.0000 | 480.0000 | 480.0000 |
| Pending Pod-Seconds | 32927778.0000 | 25995450.0000 | 43326270.0000 | 43326270.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 36.0000 | 36.0000 | 36.0000 | 36.0000 |
| Peak Cost Rate ($/hr) | 7.0340 | 7.0340 | 7.0340 | 7.0340 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 2.7840 | 2.7840 | 2.7840 | 2.7840 |
| Node Count | 11.0000 | 11.0000 | 11.0000 | 11.0000 |
| Running Pods | 22.0000 | 22.0000 | 22.0000 | 22.0000 |
| Pending Pods | 13.0000 | 13.0000 | 13.0000 | 13.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 2532973000000001.0000 | 1999715000000001.0000 | 3332860000000001.0000 | 3332860000000001.0000 |

## Variant: most_allocated

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 1907.4151 | 1546.5809 | 2577.5425 | 2577.5425 |
| Cumulative vCPU-Hours | 18655.2992 | 15253.9967 | 25259.7709 | 25259.7709 |
| Cumulative GiB-Hours | 95909.5667 | 77764.1667 | 129608.5000 | 129608.5000 |
| Time-Weighted Node Count | 27132621.0000 | 22000065.0000 | 36664605.0000 | 36664605.0000 |
| Time to Stable (s) | 480.0000 | 480.0000 | 480.0000 | 480.0000 |
| Pending Pod-Seconds | 24662460.0000 | 19996500.0000 | 33327900.0000 | 33327900.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 36.0000 | 36.0000 | 36.0000 | 36.0000 |
| Peak Cost Rate ($/hr) | 7.0340 | 7.0340 | 7.0340 | 7.0340 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 2.7840 | 2.7840 | 2.7840 | 2.7840 |
| Node Count | 11.0000 | 11.0000 | 11.0000 | 11.0000 |
| Running Pods | 25.0000 | 25.0000 | 25.0000 | 25.0000 |
| Pending Pods | 10.0000 | 10.0000 | 10.0000 | 10.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 2466317000000001.0000 | 1999715000000001.0000 | 3332860000000001.0000 | 3332860000000001.0000 |

## Comparison (Cumulative Metrics)

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| Cumulative Cost ($) | most_allocated | 51.5504 | 2.70% | 0.8174 | [-618.5770, 721.6778] |
| Cumulative vCPU-Hours | least_allocated | -3176.4292 | -17.03% | 0.2031 | [-8975.9806, 2426.1375] |
| Cumulative GiB-Hours | least_allocated | -64951.8267 | -67.72% | 0.01042 | [-91540.6800, -40955.3067] |
| Time-Weighted Node Count | most_allocated | 733260.0000 | 2.70% | 0.8174 | [-8798724.0000, 10265244.0000] |
| Time to Stable (s) | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Pending Pod-Seconds | most_allocated | 8265318.0000 | 33.51% | 0.2003 | [-1333290.0000, 17863926.0000] |
| Disruption Count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Disruption-Seconds | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Peak Node Count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Peak Cost Rate ($/hr) | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |

## Comparison (End-State Diagnostics)

| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |
|--------|--------|-------------|----------|---------|--------|
| Cost Rate ($/hr) | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Node Count | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Running Pods | least_allocated | -3.0000 | -12.00% | 0.003977 | [-3.0000, -3.0000] |
| Pending Pods | most_allocated | 3.0000 | 30.00% | 0.003977 | [3.0000, 3.0000] |
| Events Processed | tie | 0.0000 | 0.00% | 1 | [0.0000, 0.0000] |
| Final Time | most_allocated | 66656000000000.0000 | 2.70% | 0.8174 | [-799887000000000.0000, 933199000000000.0000] |
