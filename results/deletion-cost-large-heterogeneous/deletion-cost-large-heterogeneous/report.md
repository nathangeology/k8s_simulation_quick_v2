# A/B Comparison Report: deletion-cost-large-heterogeneous

Variants: baseline, largest_first, prefer_emptying, random, unallocated_vcpu  
Runs per variant: 10

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 14.1523 | 14.2240 | 14.5152 | 14.5408 |
| Cumulative vCPU-Hours | 197.3218 | 197.3710 | 202.2496 | 206.6262 |
| Cumulative GiB-Hours | 702.3998 | 703.1942 | 712.8933 | 726.9846 |
| Time-Weighted Node Count | 66339.0000 | 66675.0000 | 68040.0000 | 68160.0000 |
| Time to Stable (s) | 3303.0000 | 3225.0000 | 3540.0000 | 3540.0000 |
| Pending Pod-Seconds | 265.5000 | 60.0000 | 720.0000 | 1065.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 80.0000 | 80.0000 | 80.0000 | 80.0000 |
| Peak Cost Rate ($/hr) | 61.4400 | 61.4400 | 61.4400 | 61.4400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.6080 | 4.6080 | 4.6080 | 4.6080 |
| Node Count | 6.0000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 213.9000 | 214.0000 | 214.0000 | 214.0000 |
| Pending Pods | 0.1000 | 0.0000 | 0.0000 | 1.0000 |
| Events Processed | 873.4000 | 874.5000 | 880.0000 | 881.0000 |
| Final Time | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 14.1680 | 14.1696 | 14.5536 | 14.6944 |
| Cumulative vCPU-Hours | 197.2248 | 197.5046 | 201.1669 | 206.6653 |
| Cumulative GiB-Hours | 702.2757 | 702.9352 | 712.7191 | 725.3607 |
| Time-Weighted Node Count | 66412.5000 | 66420.0000 | 68220.0000 | 68880.0000 |
| Time to Stable (s) | 3288.0000 | 3210.0000 | 3420.0000 | 3540.0000 |
| Pending Pod-Seconds | 475.5000 | 150.0000 | 795.0000 | 2400.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 80.0000 | 80.0000 | 80.0000 | 80.0000 |
| Peak Cost Rate ($/hr) | 61.4400 | 61.4400 | 61.4400 | 61.4400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.5312 | 4.6080 | 4.6080 | 4.6080 |
| Node Count | 5.9000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 213.7000 | 214.0000 | 214.0000 | 214.0000 |
| Pending Pods | 0.3000 | 0.0000 | 1.0000 | 2.0000 |
| Events Processed | 1284.2000 | 1283.0000 | 1295.0000 | 1298.0000 |
| Final Time | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 |

## Variant: prefer_emptying

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 14.0883 | 14.1856 | 14.3104 | 14.4064 |
| Cumulative vCPU-Hours | 196.9752 | 197.3983 | 202.2488 | 206.2850 |
| Cumulative GiB-Hours | 701.4682 | 702.2442 | 711.6604 | 726.9279 |
| Time-Weighted Node Count | 66039.0000 | 66495.0000 | 67080.0000 | 67530.0000 |
| Time to Stable (s) | 3321.0000 | 3315.0000 | 3510.0000 | 3540.0000 |
| Pending Pod-Seconds | 1093.5000 | 772.5000 | 1995.0000 | 3105.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 80.0000 | 80.0000 | 80.0000 | 80.0000 |
| Peak Cost Rate ($/hr) | 61.4400 | 61.4400 | 61.4400 | 61.4400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.4544 | 4.6080 | 4.6080 | 4.6080 |
| Node Count | 5.8000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 213.3000 | 213.5000 | 214.0000 | 214.0000 |
| Pending Pods | 0.7000 | 0.5000 | 1.0000 | 3.0000 |
| Events Processed | 1282.7000 | 1280.0000 | 1295.0000 | 1301.0000 |
| Final Time | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 14.1680 | 14.1696 | 14.5536 | 14.6944 |
| Cumulative vCPU-Hours | 197.2248 | 197.5046 | 201.1669 | 206.6653 |
| Cumulative GiB-Hours | 702.2757 | 702.9352 | 712.7191 | 725.3607 |
| Time-Weighted Node Count | 66412.5000 | 66420.0000 | 68220.0000 | 68880.0000 |
| Time to Stable (s) | 3288.0000 | 3210.0000 | 3420.0000 | 3540.0000 |
| Pending Pod-Seconds | 475.5000 | 150.0000 | 795.0000 | 2400.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 80.0000 | 80.0000 | 80.0000 | 80.0000 |
| Peak Cost Rate ($/hr) | 61.4400 | 61.4400 | 61.4400 | 61.4400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.5312 | 4.6080 | 4.6080 | 4.6080 |
| Node Count | 5.9000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 213.7000 | 214.0000 | 214.0000 | 214.0000 |
| Pending Pods | 0.3000 | 0.0000 | 1.0000 | 2.0000 |
| Events Processed | 1284.2000 | 1283.0000 | 1295.0000 | 1298.0000 |
| Final Time | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 14.1043 | 14.2048 | 14.3104 | 14.4320 |
| Cumulative vCPU-Hours | 197.1402 | 197.3474 | 201.1363 | 206.3800 |
| Cumulative GiB-Hours | 702.0478 | 703.1110 | 712.4671 | 724.4729 |
| Time-Weighted Node Count | 66114.0000 | 66585.0000 | 67080.0000 | 67650.0000 |
| Time to Stable (s) | 3273.0000 | 3210.0000 | 3450.0000 | 3540.0000 |
| Pending Pod-Seconds | 603.0000 | 315.0000 | 1425.0000 | 2460.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 80.0000 | 80.0000 | 80.0000 | 80.0000 |
| Peak Cost Rate ($/hr) | 61.4400 | 61.4400 | 61.4400 | 61.4400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.6080 | 4.6080 | 4.6080 | 4.6080 |
| Node Count | 6.0000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 213.6000 | 214.0000 | 214.0000 | 214.0000 |
| Pending Pods | 0.4000 | 0.0000 | 1.0000 | 2.0000 |
| Events Processed | 1283.2000 | 1282.0000 | 1287.0000 | 1298.0000 |
| Final Time | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 | 4200000000000.0000 |
