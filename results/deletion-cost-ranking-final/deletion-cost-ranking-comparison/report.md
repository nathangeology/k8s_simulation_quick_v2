# A/B Comparison Report: deletion-cost-ranking-comparison

Variants: baseline, largest_first, random, smallest_first, unallocated_vcpu  
Runs per variant: 100

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 2284.2383 | 2239.4624 | 2559.3344 | 2879.1936 |
| time_weighted_node_count | 21414734.4000 | 20994960.0000 | 23993760.0000 | 26992440.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| peak_cost_rate | 2.7418 | 2.6880 | 3.0720 | 3.4560 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 2999281.6000 | 2999290.0000 | 2999350.0000 | 2999410.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 571.0424 | 559.8432 | 639.8208 | 719.7696 |
| time_weighted_node_count | 5353522.5000 | 5248530.0000 | 5998320.0000 | 6747840.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| peak_cost_rate | 2.7418 | 2.6880 | 3.0720 | 3.4560 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 749817.4000 | 749822.0000 | 749832.0000 | 749852.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 571.0424 | 559.8432 | 639.8208 | 719.7696 |
| time_weighted_node_count | 5353522.5000 | 5248530.0000 | 5998320.0000 | 6747840.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| peak_cost_rate | 2.7418 | 2.6880 | 3.0720 | 3.4560 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 749817.4000 | 749822.0000 | 749832.0000 | 749852.0000 |

## Variant: smallest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 571.0424 | 559.8432 | 639.8208 | 719.7696 |
| time_weighted_node_count | 5353522.5000 | 5248530.0000 | 5998320.0000 | 6747840.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| peak_cost_rate | 2.7418 | 2.6880 | 3.0720 | 3.4560 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 749817.4000 | 749822.0000 | 749832.0000 | 749852.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 571.0424 | 559.8432 | 639.8208 | 719.7696 |
| time_weighted_node_count | 5353522.5000 | 5248530.0000 | 5998320.0000 | 6747840.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| peak_cost_rate | 2.7418 | 2.6880 | 3.0720 | 3.4560 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 749817.4000 | 749822.0000 | 749832.0000 | 749852.0000 |
