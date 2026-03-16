# Kubesim Validation Report: adversarial-heterogeneous

## Setup
- **Real cluster**: KIND + KWOK + Karpenter v1.9.0 (KWOK provider, fake nodes)
- **Simulation**: kubesim with KWOK catalog, daemonsets, delays, 100 seeds
- **Scenario**: Heterogeneous workloads with mixed pod sizes (10 min)
  - 20 tiny (100m CPU, 128Mi) + 10 medium (500m, 2Gi) + 5 large (2000m, 8Gi)
  - t=3m: tiny 20→5, large 5→15
  - t=6m: all to 1 replica each

## Real Cluster Timeline

| Time | Nodes | Pods | Pending | Cost/hr | vCPU | Notes |
|------|-------|------|---------|---------|------|-------|
| 0s | 0 | 35 | 13 | $0 | 0 | Initial deploy |
| 30s | 1 | 55 | 0 | $0.76 | 15.9 | 1 node fits all 35 pods |
| 181s | 2 | 52 | 10 | $2.29 | 47.8 | t=3m: large scaled up, 2nd node needed |
| 212s | 2 | 54 | 0 | $1.72 | 35.8 | All scheduled, consolidation replaced node |
| 364s | 2 | 25 | 0 | $1.72 | 35.8 | t=6m: all scaled to 1 |
| 424s | 1 | 25 | 0 | $0.19 | 3.9 | Consolidated to 1 small node |
| 576s | 1 | 23 | 0 | $0.19 | 3.9 | Stable end state |

## Sim vs Real Comparison

| Metric | Real | Sim Mean | Sim p5 | Sim p95 | Percentile | Status |
|--------|------|----------|--------|---------|------------|--------|
| Cumulative Cost ($) | 0.16 | 0.06 | 0.06 | 0.06 | p100 | ❌ OUTLIER |
| Peak Nodes | 2 | 10 | 10 | 10 | p0 | ❌ OUTLIER |
| End-State Nodes | 1 | 1 | 1 | 1 | match | ✅ OK |
| End-State Cost ($/hr) | 0.19 | 0.12 | 0.12 | 0.12 | p100 | ❌ OUTLIER |

## Key Findings

### ✅ What matches well
1. **End-state convergence** — both reach exactly 1 node after consolidation (100% match)
2. **Phase structure** — both show the same 3-phase lifecycle (initial → scale shift → consolidate)

### ⚠️ Fidelity gaps
1. **Instance type selection (primary gap)**: Real KWOK used 1-2 large nodes (16-48 vCPU). Sim consistently used 10 smaller nodes. This explains the cost and node count divergence.
2. **Cumulative cost**: Real=$0.16, Sim=$0.06 (156% diff). Real cluster's large nodes cost more per hour but the sim's many small nodes have lower per-node cost. The real cluster also held 2 nodes longer before consolidating.
3. **Sim determinism**: Sim showed zero variance (p5=p95 for all metrics). With fixed pod sizes (no distribution), the sim produces identical results every run. Real cluster has inherent timing jitter.
4. **Bin-packing efficiency**: Real Karpenter packed all 35 initial pods (mixed sizes) onto 1 node. Sim spread across 10 nodes — suggests sim's bin-packing is less aggressive or uses different instance selection.

### 🔬 Observations
- The heterogeneous workload (100m to 2000m CPU) tests bin-packing. Real Karpenter selected a single large node that fit everything; sim used many small nodes.
- When large pods scaled from 5→15 at t=3m, real cluster needed only 1 additional node. Sim already had 10 nodes.
- Consolidation after t=6m took ~60s in real (2→1 nodes), matching the 30s consolidateAfter + drain time.
- The 10 pending pods at t=181s show real provisioning latency for the 2nd node.
