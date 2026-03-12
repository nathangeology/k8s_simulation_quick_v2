# A/B Comparison Report: deletion-cost-heterogeneous

Variants: baseline, largest_first, prefer_emptying, random, unallocated_vcpu  
Runs per variant: 5

## Variant: baseline

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 31998.1120 | 31998.1120 | 31998.1120 | 31998.1120 |
| time_weighted_node_count | 149991150.0000 | 149991150.0000 | 149991150.0000 | 149991150.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| peak_cost_rate | 3.8400 | 3.8400 | 3.8400 | 3.8400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.8400 | 3.8400 | 3.8400 | 3.8400 |
| node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| running_pods | 35.0000 | 35.0000 | 35.0000 | 35.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| final_time | 29998240.0000 | 29998240.0000 | 29998240.0000 | 29998240.0000 |

## Variant: largest_first

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 7999.5200 | 7999.5200 | 7999.5200 | 7999.5200 |
| time_weighted_node_count | 37497750.0000 | 37497750.0000 | 37497750.0000 | 37497750.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| peak_cost_rate | 3.8400 | 3.8400 | 3.8400 | 3.8400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.8400 | 3.8400 | 3.8400 | 3.8400 |
| node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| running_pods | 35.0000 | 35.0000 | 35.0000 | 35.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| final_time | 7499560.0000 | 7499560.0000 | 7499560.0000 | 7499560.0000 |

## Variant: prefer_emptying

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 7999.5200 | 7999.5200 | 7999.5200 | 7999.5200 |
| time_weighted_node_count | 37497750.0000 | 37497750.0000 | 37497750.0000 | 37497750.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| peak_cost_rate | 3.8400 | 3.8400 | 3.8400 | 3.8400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.8400 | 3.8400 | 3.8400 | 3.8400 |
| node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| running_pods | 35.0000 | 35.0000 | 35.0000 | 35.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| final_time | 7499560.0000 | 7499560.0000 | 7499560.0000 | 7499560.0000 |

## Variant: random

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 7999.5200 | 7999.5200 | 7999.5200 | 7999.5200 |
| time_weighted_node_count | 37497750.0000 | 37497750.0000 | 37497750.0000 | 37497750.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| peak_cost_rate | 3.8400 | 3.8400 | 3.8400 | 3.8400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.8400 | 3.8400 | 3.8400 | 3.8400 |
| node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| running_pods | 35.0000 | 35.0000 | 35.0000 | 35.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| final_time | 7499560.0000 | 7499560.0000 | 7499560.0000 | 7499560.0000 |

## Variant: unallocated_vcpu

### Cumulative Metrics (Primary)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| cumulative_cost | 7999.5200 | 7999.5200 | 7999.5200 | 7999.5200 |
| time_weighted_node_count | 37497750.0000 | 37497750.0000 | 37497750.0000 | 37497750.0000 |
| time_to_stable | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| cumulative_pending_pod_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_count | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| disruption_seconds | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| peak_node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| peak_cost_rate | 3.8400 | 3.8400 | 3.8400 | 3.8400 |

### End-State Metrics (Diagnostic)

| Metric | Mean | Median | p90 | p99 |
|--------|------|--------|-----|-----|
| total_cost_per_hour | 3.8400 | 3.8400 | 3.8400 | 3.8400 |
| node_count | 5.0000 | 5.0000 | 5.0000 | 5.0000 |
| running_pods | 35.0000 | 35.0000 | 35.0000 | 35.0000 |
| pending_pods | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| events_processed | 1000000.0000 | 1000000.0000 | 1000000.0000 | 1000000.0000 |
| final_time | 7499560.0000 | 7499560.0000 | 7499560.0000 | 7499560.0000 |
