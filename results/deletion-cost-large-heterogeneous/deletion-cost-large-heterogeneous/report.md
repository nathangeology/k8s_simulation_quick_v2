# A/B Comparison Report: deletion-cost-large-heterogeneous

Variants: baseline, largest_first, prefer_emptying, random, unallocated_vcpu  
Runs per variant: 5

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 3176.8458 | 3208.7771 | 3563.9875 | 3563.9875 |
| time_weighted_node_count | 15694074.0000 | 15833370.0000 | 16707780.0000 | 16707780.0000 |
| time_to_stable | 3552.0000 | 3540.0000 | 4020.0000 | 4020.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 22.2000 | 22.0000 | 23.0000 | 23.0000 |
| peak_cost_rate | 15.6672 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 4.3688 | 4.6080 | 4.6080 | 4.6080 |
| node_count | 6.0000 | 6.0000 | 6.0000 | 6.0000 |
| running_pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 5000000.0000 | 5000000.0000 | 5000000.0000 | 5000000.0000 |
| final_time | 2609952000000003.0000 | 2633300000000004.0000 | 2779510000000004.0000 | 2779510000000004.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 3332.8620 | 3376.9678 | 3564.1228 | 3564.1228 |
| time_weighted_node_count | 16057152.0000 | 16706640.0000 | 16707900.0000 | 16707900.0000 |
| time_to_stable | 3372.0000 | 3360.0000 | 3750.0000 | 3750.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 21.0000 | 21.0000 | 23.0000 | 23.0000 |
| peak_cost_rate | 15.6672 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 4.4884 | 4.6080 | 4.6080 | 4.6080 |
| node_count | 6.0000 | 6.0000 | 6.0000 | 6.0000 |
| running_pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 5000000.0000 | 5000000.0000 | 5000000.0000 | 5000000.0000 |
| final_time | 2670828000000003.5000 | 2779380000000004.0000 | 2779470000000004.0000 | 2779470000000004.0000 |

## Variant: prefer_emptying

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 2937.3055 | 2940.3913 | 3208.9861 | 3208.9861 |
| time_weighted_node_count | 14946378.0000 | 15047565.0000 | 15834960.0000 | 15834960.0000 |
| time_to_stable | 3720.0000 | 3780.0000 | 4050.0000 | 4050.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 22.0000 | 22.0000 | 23.0000 | 23.0000 |
| peak_cost_rate | 15.6672 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 4.2492 | 4.0100 | 4.6080 | 4.6080 |
| node_count | 6.0000 | 6.0000 | 6.0000 | 6.0000 |
| running_pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 5000000.0000 | 5000000.0000 | 5000000.0000 | 5000000.0000 |
| final_time | 2485076000000002.5000 | 2501690000000001.0000 | 2633340000000001.0000 | 2633340000000001.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 3363.3269 | 3377.0190 | 3564.1228 | 3564.1228 |
| time_weighted_node_count | 16199886.0000 | 16706640.0000 | 16707900.0000 | 16707900.0000 |
| time_to_stable | 3414.0000 | 3360.0000 | 3750.0000 | 3750.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 20.8000 | 21.0000 | 22.0000 | 22.0000 |
| peak_cost_rate | 15.6672 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 4.4884 | 4.6080 | 4.6080 | 4.6080 |
| node_count | 6.0000 | 6.0000 | 6.0000 | 6.0000 |
| running_pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 5000000.0000 | 5000000.0000 | 5000000.0000 | 5000000.0000 |
| final_time | 2694626000000004.0000 | 2779380000000004.0000 | 2779470000000004.0000 | 2779470000000004.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 3147.4531 | 3208.5906 | 3563.9395 | 3563.9395 |
| time_weighted_node_count | 15166461.0000 | 15046275.0000 | 16709190.0000 | 16709190.0000 |
| time_to_stable | 3522.0000 | 3480.0000 | 3780.0000 | 3780.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 21.0000 | 21.0000 | 22.0000 | 22.0000 |
| peak_cost_rate | 15.6672 | 15.3600 | 16.1280 | 16.1280 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 4.3348 | 4.6080 | 4.6080 | 4.6080 |
| node_count | 5.8000 | 6.0000 | 6.0000 | 6.0000 |
| running_pods | 134.0000 | 134.0000 | 134.0000 | 134.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 5000000.0000 | 5000000.0000 | 5000000.0000 | 5000000.0000 |
| final_time | 2609850000000003.0000 | 2633240000000001.0000 | 2779420000000004.0000 | 2779420000000004.0000 |
