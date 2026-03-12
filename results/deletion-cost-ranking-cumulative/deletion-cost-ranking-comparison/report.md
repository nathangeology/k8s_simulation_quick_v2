# A/B Comparison Report: deletion-cost-ranking-comparison

Variants: baseline, largest_first, random, smallest_first, unallocated_vcpu  
Runs per variant: 100

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_weighted_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
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
| final_time | 2999271600000000.5000 | 2999280000000001.0000 | 2999340000000001.0000 | 2999400000000001.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_weighted_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_cost_rate | 0.0000 | 0.0000 | 0.0000 | 0.0000 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 999759.2000 | 999762.0000 | 999782.0000 | 999802.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_weighted_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_cost_rate | 0.0000 | 0.0000 | 0.0000 | 0.0000 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 999759.2000 | 999762.0000 | 999782.0000 | 999802.0000 |

## Variant: smallest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_weighted_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_cost_rate | 0.0000 | 0.0000 | 0.0000 | 0.0000 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 999759.2000 | 999762.0000 | 999782.0000 | 999802.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_weighted_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_cost_rate | 0.0000 | 0.0000 | 0.0000 | 0.0000 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 2.7418 | 2.6880 | 3.0720 | 3.4560 |
| node_count | 7.1400 | 7.0000 | 8.0000 | 9.0000 |
| running_pods | 15.0000 | 15.0000 | 15.0000 | 15.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 100000.0000 | 100000.0000 | 100000.0000 | 100000.0000 |
| final_time | 999759.2000 | 999762.0000 | 999782.0000 | 999802.0000 |
