# A/B Comparison Report: chaos-000

Variants: baseline, largest_first, random, smallest_first, unallocated_vcpu  
Runs per variant: 5

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 5666.5885 | 5666.5873 | 5666.5930 | 5666.5930 |
| Cumulative vCPU-Hours | 123636.6278 | 131531.6234 | 132889.6733 | 132889.6733 |
| Cumulative GiB-Hours | 196311.8073 | 194613.8052 | 266608.8334 | 266608.8334 |
| Time-Weighted Node Count | 29999586.0000 | 29999580.0000 | 29999610.0000 | 29999610.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 13391815278.0000 | 13469824890.0000 | 13529810580.0000 | 13529810580.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Peak Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |
| Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Running Pods | 8.6000 | 6.0000 | 18.0000 | 18.0000 |
| Pending Pods | 446.4000 | 449.0000 | 451.0000 | 451.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 29999596.0000 | 29999590.0000 | 29999620.0000 | 29999620.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 1416.6440 | 1416.6440 | 1416.6440 | 1416.6440 |
| Cumulative vCPU-Hours | 30909.0888 | 32882.8072 | 33222.3851 | 33222.3851 |
| Cumulative GiB-Hours | 49077.8374 | 48653.3053 | 66652.0084 | 66652.0084 |
| Time-Weighted Node Count | 7499880.0000 | 7499880.0000 | 7499880.0000 | 7499880.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 3347946432.0000 | 3367446120.0000 | 3382445880.0000 | 3382445880.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Peak Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |
| Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Running Pods | 8.6000 | 6.0000 | 18.0000 | 18.0000 |
| Pending Pods | 446.4000 | 449.0000 | 451.0000 | 451.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 7499895.6000 | 7499892.0000 | 7499902.0000 | 7499902.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 1416.6440 | 1416.6440 | 1416.6440 | 1416.6440 |
| Cumulative vCPU-Hours | 30909.0888 | 32882.8072 | 33222.3851 | 33222.3851 |
| Cumulative GiB-Hours | 49077.8374 | 48653.3053 | 66652.0084 | 66652.0084 |
| Time-Weighted Node Count | 7499880.0000 | 7499880.0000 | 7499880.0000 | 7499880.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 3347946432.0000 | 3367446120.0000 | 3382445880.0000 | 3382445880.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Peak Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |
| Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Running Pods | 8.6000 | 6.0000 | 18.0000 | 18.0000 |
| Pending Pods | 446.4000 | 449.0000 | 451.0000 | 451.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 7499895.6000 | 7499892.0000 | 7499902.0000 | 7499902.0000 |

## Variant: smallest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 1416.6440 | 1416.6440 | 1416.6440 | 1416.6440 |
| Cumulative vCPU-Hours | 30909.0888 | 32882.8072 | 33222.3851 | 33222.3851 |
| Cumulative GiB-Hours | 49077.8374 | 48653.3053 | 66652.0084 | 66652.0084 |
| Time-Weighted Node Count | 7499880.0000 | 7499880.0000 | 7499880.0000 | 7499880.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 3347946432.0000 | 3367446120.0000 | 3382445880.0000 | 3382445880.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Peak Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |
| Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Running Pods | 8.6000 | 6.0000 | 18.0000 | 18.0000 |
| Pending Pods | 446.4000 | 449.0000 | 451.0000 | 451.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 7499895.6000 | 7499892.0000 | 7499902.0000 | 7499902.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 1416.6440 | 1416.6440 | 1416.6440 | 1416.6440 |
| Cumulative vCPU-Hours | 30909.0888 | 32882.8072 | 33222.3851 | 33222.3851 |
| Cumulative GiB-Hours | 49077.8374 | 48653.3053 | 66652.0084 | 66652.0084 |
| Time-Weighted Node Count | 7499880.0000 | 7499880.0000 | 7499880.0000 | 7499880.0000 |
| Time to Stable (s) | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pending Pod-Seconds | 3347946432.0000 | 3367446120.0000 | 3382445880.0000 | 3382445880.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Peak Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 0.6800 | 0.6800 | 0.6800 | 0.6800 |
| Node Count | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| Running Pods | 8.6000 | 6.0000 | 18.0000 | 18.0000 |
| Pending Pods | 446.4000 | 449.0000 | 451.0000 | 451.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 7499895.6000 | 7499892.0000 | 7499902.0000 | 7499902.0000 |
