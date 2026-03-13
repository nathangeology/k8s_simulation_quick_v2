# A/B Comparison Report: deletion-cost-large-heterogeneous

Variants: baseline, largest_first, prefer_emptying, random, unallocated_vcpu  
Runs per variant: 10

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 647.6225 | 648.6959 | 682.1412 | 761.3516 |
| Cumulative vCPU-Hours | 11820.6337 | 11512.6965 | 12883.5358 | 13111.0374 |
| Cumulative GiB-Hours | 43958.8996 | 43339.6954 | 47658.7088 | 48776.3426 |
| Time-Weighted Node Count | 3207132.0000 | 3199132.5000 | 3569700.0000 | 3571275.0000 |
| Time to Stable (s) | 3495.0000 | 3510.0000 | 3780.0000 | 3780.0000 |
| Pending Pod-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 21.5000 | 21.5000 | 23.0000 | 23.0000 |
| Peak Cost Rate ($/hr) | 15.4368 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.2942 | 4.3200 | 4.6080 | 4.6080 |
| Node Count | 5.9000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| Pending Pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 538045000000004.0000 | 527975000000004.0000 | 589960000000004.0000 | 590010000000004.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 619.3918 | 641.6562 | 648.4968 | 719.2800 |
| Cumulative vCPU-Hours | 11342.4536 | 11224.3041 | 12271.9665 | 12737.0885 |
| Cumulative GiB-Hours | 42175.2125 | 42016.7910 | 45131.9472 | 48571.1047 |
| Time-Weighted Node Count | 3023125.5000 | 3042375.0000 | 3372345.0000 | 3372840.0000 |
| Time to Stable (s) | 3456.0000 | 3420.0000 | 3750.0000 | 3840.0000 |
| Pending Pod-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 21.4000 | 21.5000 | 22.0000 | 25.0000 |
| Peak Cost Rate ($/hr) | 15.4368 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.2772 | 4.3200 | 4.6080 | 4.6080 |
| Node Count | 5.8000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| Pending Pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 516376000000003.5000 | 501675000000004.0000 | 557170000000004.0000 | 589770000000004.0000 |

## Variant: prefer_emptying

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 609.7520 | 597.6123 | 681.8110 | 682.2291 |
| Cumulative vCPU-Hours | 11377.7162 | 11464.9970 | 11746.0282 | 12074.6874 |
| Cumulative GiB-Hours | 42319.3880 | 41987.9761 | 43694.5184 | 45902.8469 |
| Time-Weighted Node Count | 2982108.0000 | 3044205.0000 | 3200370.0000 | 3201990.0000 |
| Time to Stable (s) | 3555.0000 | 3495.0000 | 3780.0000 | 3780.0000 |
| Pending Pod-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 21.3000 | 21.0000 | 22.0000 | 24.0000 |
| Peak Cost Rate ($/hr) | 15.4368 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.1982 | 4.0100 | 4.6080 | 4.6080 |
| Node Count | 5.7000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| Pending Pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 517945000000003.5000 | 527915000000003.0000 | 528030000000001.0000 | 557140000000004.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 622.7480 | 641.6999 | 681.8596 | 719.2800 |
| Cumulative vCPU-Hours | 11399.9162 | 11478.1316 | 12271.9665 | 12737.0885 |
| Cumulative GiB-Hours | 42386.3236 | 42676.9964 | 45131.9472 | 48571.1047 |
| Time-Weighted Node Count | 3038857.5000 | 3043320.0000 | 3372345.0000 | 3372840.0000 |
| Time to Stable (s) | 3456.0000 | 3420.0000 | 3750.0000 | 3840.0000 |
| Pending Pod-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 21.4000 | 21.5000 | 22.0000 | 25.0000 |
| Peak Cost Rate ($/hr) | 15.4368 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.2772 | 4.3200 | 4.6080 | 4.6080 |
| Node Count | 5.8000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| Pending Pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 518998000000003.5000 | 514765000000004.0000 | 557170000000004.0000 | 589770000000004.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cumulative Cost ($) | 632.8803 | 624.5480 | 719.3740 | 760.9536 |
| Cumulative vCPU-Hours | 11610.2822 | 11835.7812 | 12386.8682 | 13049.8560 |
| Cumulative GiB-Hours | 43197.6821 | 43907.5498 | 46080.3167 | 48606.8050 |
| Time-Weighted Node Count | 3044487.0000 | 3040912.5000 | 3375990.0000 | 3566970.0000 |
| Time to Stable (s) | 3408.0000 | 3360.0000 | 3780.0000 | 3780.0000 |
| Pending Pod-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption Count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Disruption-Seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Peak Node Count | 21.1000 | 21.5000 | 22.0000 | 23.0000 |
| Peak Cost Rate ($/hr) | 15.4368 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| Cost Rate ($/hr) | 4.2580 | 4.3090 | 4.6080 | 4.6080 |
| Node Count | 5.7000 | 6.0000 | 6.0000 | 6.0000 |
| Running Pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| Pending Pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Events Processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| Final Time | 528323000000004.0000 | 542480000000004.0000 | 557250000000004.0000 | 589800000000004.0000 |
