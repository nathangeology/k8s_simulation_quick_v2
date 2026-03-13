# Kubesim Validation Report: adversarial-deletion-cost

## Setup
- **Real cluster**: KIND + KWOK + Karpenter v1.9.0 (KWOK provider, fake nodes)
- **Simulation**: kubesim with KWOK catalog, daemonsets, delays, 100 seeds
- **Scenario**: Deletion cost ranking — which nodes get drained first (15 min)
  - 100 pods (500m CPU each) across nodes
  - t=5m: scale down to 50
  - t=10m: scale down to 10

## Real Cluster Timeline

| Time | Nodes | Pods | Pending | Cost/hr | vCPU | Notes |
|------|-------|------|---------|---------|------|-------|
| 0s | 0 | 36 | 15 | $0 | 0 | Initial deploy |
| 30s | 1 | 122 | 0 | $3.07 | 63.9 | 1 large node fits all 100 pods |
| 30-273s | 1 | 120 | 0 | $3.07 | 63.9 | Stable — all on 1 node |
| 303s | 1 | 70 | 0 | $3.07 | 63.9 | t=5m: scaled to 50, pods draining |
| 363s | 1 | 72 | 0 | $1.53 | 31.9 | Consolidated to smaller node |
| 606s | 1 | 30 | 0 | $1.53 | 31.9 | t=10m: scaled to 10, pods draining |
| 667s | 1 | 32 | 0 | $0.19 | 3.9 | Consolidated to smallest node |
| 879s | 1 | 30 | 0 | $0.19 | 3.9 | Stable end state |

## Sim vs Real Comparison

| Metric | Real | Sim Mean | Sim p5 | Sim p95 | Percentile | Status |
|--------|------|----------|--------|---------|------------|--------|
| Cumulative Cost ($) | 0.43 | 0.53 | 0.53 | 0.53 | p0 | ❌ OUTLIER |
| Peak Nodes | 1 | 3 | 3 | 3 | p0 | ❌ OUTLIER |
| End-State Nodes | 1 | 1 | 1 | 1 | match | ✅ OK |
| End-State Cost ($/hr) | 0.19 | 1.30 | 1.30 | 1.30 | p0 | ❌ OUTLIER |

## Key Findings

### ✅ What matches well
1. **End-state convergence** — both reach exactly 1 node (100% match)
2. **Consolidation behavior** — real cluster right-sized nodes at each scale-down step (64→32→4 vCPU)
3. **Phase structure** — both show 3 distinct cost plateaus matching the 3 scale phases

### ⚠️ Fidelity gaps
1. **Node count (primary gap)**: Real=1 node throughout (KWOK selected a single 64-vCPU node for 100×500m pods). Sim used 3 nodes. The real cluster never needed multi-node consolidation decisions.
2. **End-state cost**: Real=$0.19/hr, Sim=$1.30/hr. Sim's end-state node is much larger than needed. Suggests sim doesn't right-size (replace with smaller node) as aggressively as real Karpenter.
3. **Consolidation right-sizing**: Real Karpenter replaced the 64-vCPU node with a 32-vCPU node at t=363s, then a 4-vCPU node at t=667s. This "replace with smaller" behavior is key to cost efficiency.
4. **Sim determinism**: Zero variance in sim (p5=p95). Fixed pod sizes produce identical sim runs.

### 🔬 Deletion Cost Observations
- **Node drain ordering was not testable** in this run because real Karpenter fit all 100 pods on a single large KWOK node. With only 1 node, there's no multi-node drain ordering to observe.
- To properly test deletion cost ranking, the NodePool should be restricted to smaller instance types (e.g., max 8 vCPU) to force pods across 10+ nodes.
- The sim's 3-node topology would have been a better test of drain ordering. The instance type mismatch ironically made the real cluster less interesting for this scenario.

### Recommended Follow-up
- Re-run with NodePool `requirements` restricting to small instance types (c-1x, c-2x, c-4x only) to force multi-node topology
- This would create ~13 nodes for 100 pods, enabling meaningful drain ordering comparison
