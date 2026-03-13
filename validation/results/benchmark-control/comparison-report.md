# Kubesim Validation Report: benchmark-control

## Setup
- **Real cluster**: KIND + KWOK + Karpenter v1.9.0 (KWOK provider, fake nodes)
- **Simulation**: kubesim with EKS overhead + daemonsets, 100 seeds
- **Scenario**: benchmark-control (2 deployments, scale 1→500→350→10 each, 40 min)

## Instance Type Mismatch (Primary Gap)

The KWOK provider selected large instance types (~80 vCPU, ~450 GiB per node) from its
built-in catalog, while the sim uses m5.xlarge (4 vCPU) and m5.2xlarge (8 vCPU). This
means the real cluster needed only 11 nodes vs 200 in the sim for the same 1000 pods.

**Action needed**: Either restrict KWOK NodePool to m5-equivalent types, or add KWOK's
instance type catalog to the sim for apples-to-apples comparison.

## Phase Comparison

| Phase | Real (Karpenter 1.9) | Sim (kubesim) | Delta |
|-------|---------------------|---------------|-------|
| Initial | 0 nodes, 2 pods | 0 nodes, 2 pods | Match ✅ |
| Scale-out peak | 11 nodes, 1040 pods, 96 pending | 200 nodes, 1200 pods, 0 pending | Instance types differ |
| Scale-out time | ~60s to peak | ~10s to peak | Real is slower (batching) |
| Post scale-down #1 (t=15m) | 11→11 nodes, 756 pods | 200→189 nodes, 1200 pods | Consolidation timing differs |
| Post scale-down #2 (t=25m) | 4→1 nodes, 40 pods | 133→10 nodes, 220 pods | Both consolidate well |
| End state | 1 node, 40 pods, $1.53/hr | 10 nodes, 220 pods, $3.26/hr | Pod count differs (daemonsets) |

## Cumulative Metrics

| Metric | Real | Sim (mean) | Notes |
|--------|------|-----------|-------|
| Cumulative cost | $17.47 | $33.58 | Different instance sizes |
| Cumulative vCPU-hours | 364.0 | 354.4 | Very close! ✅ |
| Peak nodes | 11 | 200 | Instance type mismatch |
| End nodes | 1 | 10 | Sim has daemonset pods |
| End pods | 40 | 220 | 200 daemonset + 20 workload |

## Key Findings

### ✅ What matches well
1. **vCPU-hours are nearly identical** (364 real vs 354 sim) — total compute consumed is accurate
2. **Consolidation timing** — both reach stable state ~10-15 min after final scale-down
3. **Phase structure** — both show the same 4-phase lifecycle pattern
4. **Scale-down responsiveness** — consolidation kicks in within 30-60s of scale-down

### ⚠️ Fidelity gaps to address
1. **Instance type selection**: KWOK uses large types (80+ vCPU), sim uses m5 (4-8 vCPU).
   Fix: restrict NodePool `requirements` to specific instance types matching sim config.
2. **Provisioning speed**: Real Karpenter takes ~60s to provision all nodes (batching + API latency),
   sim provisions instantly. Fix: add provisioning delay to sim.
3. **Pending pods**: Real cluster had 96 pods stuck pending (CPU limit 800 < 1000 pods × 950m).
   Sim had 0 pending (200 × 4 vCPU = 800 vCPU, but FFD packing fits more). Fix: verify
   NodePool CPU limits match between real and sim.
4. **Daemonset pods**: Sim creates 200 daemonset pods (1 per node), real cluster had none
   (KWOK nodes don't run daemonsets by default). Fix: either disable daemonsets in sim
   for KWOK comparison, or configure KWOK to simulate daemonsets.
5. **End-state node count**: Real=1, Sim=10. The real cluster's large nodes can fit all
   remaining pods on 1 node. Sim's small nodes need 10.

### 🔬 Interesting observations
- Real Karpenter's batch provisioner creates nodes in waves (2→8→11 over 60s), while
  sim creates all nodes in one shot. This affects the provisioning cost curve.
- Consolidation in real Karpenter is more aggressive — went from 11→1 nodes, while sim
  went from 200→10. Both are correct for their instance sizes.
- The `consolidateAfter: 30s` setting in real cluster matches sim's 30s base interval.

## Next Steps
1. Re-run with NodePool restricted to m5.xlarge/m5.2xlarge equivalent KWOK types
2. Add provisioning delay to sim (configurable, default ~30s)
3. Run adversarial scenarios on real cluster
4. Compare consolidation decision ordering (which nodes get drained first)
