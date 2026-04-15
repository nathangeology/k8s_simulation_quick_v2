# KWOK 3-Variant Balanced Threshold Verification

## Parameters
- 500 replicas × 2 deployments, scale: 500→350 at 15min, 350→10 at 25min
- consolidateAfter: 30s, full heterogeneous KWOK fleet (amd64+linux)
- 60s metrics intervals, 35min total per variant

## Results

| Variant | Disrupted Nodes | Pod Evictions | Final Nodes | Empty Path | Underutil Path | Balanced Path |
|---------|----------------|---------------|-------------|------------|----------------|---------------|
| when-empty | 3 | 0 | 5 | 3 | 0 | 0 |
| balanced-k2 | 8 | 0 | 1 | 2 | 0 | 6 |
| when-underutilized | 9 | 0 | 1 | 3 | 5 | 1 |

## Window Analysis

| Variant | W1 Before (500→350) | W1 After | W2 Before (350→10) | Final |
|---------|---------------------|----------|--------------------|-------|
| when-empty | 8 | 8 | 8 | 5 |
| balanced-k2 | 6 | 4 | 4 | 1 |
| when-underutilized | 8 | 4 | 4 | 1 |

## Sim vs KWOK Comparison

| Variant | Sim Disruptions | KWOK Disruptions | Sim Final Nodes | KWOK Final Nodes |
|---------|----------------|------------------|-----------------|------------------|
| when-empty | 0 | 3 | 3.6 | 5 |
| balanced-k2 | 140 | 8 | 1.0 | 1 |
| when-underutilized | 516 | 9 | 1.0 | 1 |
