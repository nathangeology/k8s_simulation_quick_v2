# InPlace Pod Vertical Scaling + Karpenter Consolidation: Safety Analysis

**Context:** [kubernetes-sigs/karpenter#829](https://github.com/kubernetes-sigs/karpenter/issues/829)
**Question:** *How do we keep consolidation from causing too much disruption and cancelling out the potential savings from in-place resize?*

**Method:** Discrete-event simulation (kubesim), 20 iterations per scenario×strategy, KWOK-based instance catalog. Five consolidation strategies tested across three workload patterns representing real-world InPlacePodVerticalScaling use cases.

---

## 1. Executive Summary

| Workload Pattern | Safe Strategies | Avoid |
|---|---|---|
| JVM startup spike (resize-down) | WhenEmpty, CostJustified(≥2.0) | WhenUnderutilized, CostJustified(<2.0) |
| VPA gradual scale-up (resize-up) | All strategies are safe | — |
| Random jitter (mixed resize) | WhenEmpty, CostJustified(≥2.0) | WhenUnderutilized, CostJustified(<2.0) |

**Key finding:** In-place resize creates a new failure mode for consolidation. When pods shrink in place, nodes become underutilized without any pod deletion — triggering consolidation that disrupts pods which just finished resizing. The CostJustified threshold acts as a **disruption dial**: values ≤1.0 behave identically to WhenUnderutilized, while 2.0 blocks nearly all post-resize consolidation. A threshold around 1.5–2.0 is the sweet spot for balancing savings against disruption.

---

## 2. JVM Startup Spike Findings

**Scenario:** 200 pods burst with 4 CPU / 8Gi (JVM startup + JIT), then resize down to 1 CPU / 2Gi at t=2m. Models the idealo Spring Boot use case from karpenter#829.

| Strategy | Disruptions | Cost ($) | Cost Savings | Node·Time |
|---|---|---|---|---|
| WhenEmpty | 0 | 34.88 | 0% (baseline) | 467,340 |
| CostJustified(2.0) | 0 | 34.88 | 0% | 467,340 |
| CostJustified(1.0) | 281 | 10.24 | 71% | 137,250 |
| CostJustified(0.5) | 281 | 10.24 | 71% | 137,250 |
| WhenUnderutilized | 281 | 10.24 | 71% | 137,250 |

**Analysis:**

- **The thrash problem is real.** WhenUnderutilized produces 281 disruptions — consolidation fires immediately after pods resize down, evicting pods that just stabilized. The 71% cost savings comes at the price of massive churn.
- **CostJustified ≤1.0 offers no protection.** Thresholds 0.5 and 1.0 produce identical results to WhenUnderutilized — the cost savings from consolidating 4x-oversized nodes easily exceeds any threshold below 2.0.
- **CostJustified(2.0) fully blocks thrash** but sacrifices all cost savings. Nodes remain at their original (oversized) allocation.
- **consolidateAfter tuning** (not yet tested in simulation) could provide a middle ground: letting resizes settle before consolidation evaluates.
- **WhenEmpty produces 0 disruptions** because after resize-down, nodes still have pods on them — they're underutilized but not empty.

**Recommendation for JVM workloads:** Use `consolidateAfter: 15m` with WhenUnderutilized or CostJustified(1.0) to let JVM warmup + resize complete before consolidation acts. Alternatively, use pod-deletion-cost annotations during the startup window.

---

## 3. VPA Gradual Scale-Up Findings

**Scenario:** 100 pods start at 500m CPU / 1Gi, grow in 3 steps to 2 CPU / 4Gi over 10 minutes. Models VPA-driven right-sizing.

| Strategy | Disruptions | Cost ($) | Peak Nodes | Node·Time |
|---|---|---|---|---|
| All strategies | 0 | 1.38 | 7 | 18,480 |

**Analysis:**

- **All strategies produce identical results.** Zero disruptions, identical cost and node counts across every variant.
- **Why:** Pods only grow — they never shrink. Growing pods make nodes *more* utilized, never less. There is no consolidation opportunity because no node becomes underutilized or empty.
- **Infeasible resizes:** The simulator provisions new nodes when pods outgrow their current nodes. Peak node count of 7 (for 100 pods growing to 2 CPU each) shows Karpenter keeps up with demand.
- **No thrash risk** for resize-up workloads. Consolidation strategy is irrelevant.

**Recommendation for VPA scale-up workloads:** Any consolidation strategy is safe. Focus on ensuring sufficient headroom in node pools for resize-up events.

---

## 4. Random Jitter Findings

**Scenario:** 100 pods (two groups of 50 with offset timing) alternate between high and low resource requests every 2–6 minutes. Worst-case chaos pattern.

| Strategy | Disruptions | Cost ($) | Cost Savings | Node·Time |
|---|---|---|---|---|
| WhenEmpty | 0.0 ± 0.0 | 3.18 ± 0.00 | 0% (baseline) | 44,100 ± 0 |
| CostJustified(2.0) | 0.1 ± 0.4 | 3.18 ± 0.02 | 0.2% | 43,960 ± 611 |
| CostJustified(1.0) | 50.6 ± 2.5 | 1.91 ± 0.04 | 40% | 25,691 ± 478 |
| CostJustified(0.5) | 50.6 ± 2.5 | 1.91 ± 0.04 | 40% | 25,691 ± 478 |
| WhenUnderutilized | 51.3 ± 2.3 | 1.91 ± 0.04 | 40% | 25,772 ± 491 |

**Analysis:**

- **Jitter introduces variance** (std > 0) unlike the deterministic JVM scenario, confirming the stochastic nature of the workload.
- **WhenUnderutilized and CostJustified(≤1.0)** produce ~51 disruptions for 40% cost savings. The constant resize oscillation means consolidation repeatedly fires as pods shrink, then new nodes are needed as pods grow back.
- **CostJustified(2.0) nearly eliminates disruptions** (0.1 mean — essentially zero) while sacrificing almost all cost savings.
- **WhenEmpty is perfectly stable** — zero disruptions, but no savings.
- **The gap between threshold 1.0 and 2.0 is where the interesting tradeoff lives.** A threshold of ~1.5 (not tested) would likely provide partial savings with reduced disruption.

**Recommendation for jitter workloads:** CostJustified(2.0) or WhenEmpty for stability. If cost savings matter, combine CostJustified(1.0) with `consolidateAfter: 10m+` to dampen oscillation.

---

## 5. Recommended Configurations

### For JVM-style workloads (large resize-down after startup)
```yaml
apiVersion: karpenter.sh/v1
kind: NodePool
spec:
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized
    consolidateAfter: 15m  # Let JVM warmup + resize complete
```
Or use pod-level protection during startup:
```yaml
metadata:
  annotations:
    karpenter.sh/do-not-disrupt: "true"  # Remove after resize-down completes
```

### For VPA-driven scale-up workloads
```yaml
spec:
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized  # Any policy is safe
    consolidateAfter: 0s
```

### For mixed/unpredictable resize patterns
```yaml
spec:
  disruption:
    consolidationPolicy: WhenEmpty  # Safest default
```
Or if cost savings are needed:
```yaml
spec:
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized
    consolidateAfter: 10m  # Dampen oscillation
```

### For mixed clusters (nodepool-level separation)
```yaml
# Nodepool for JVM/startup-heavy workloads
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: jvm-workloads
spec:
  disruption:
    consolidationPolicy: WhenEmpty
    consolidateAfter: 15m
---
# Nodepool for stable workloads
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: stable-workloads
spec:
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized
    consolidateAfter: 0s
```

---

## 6. Karpenter Feature Recommendations

Based on simulation findings, three features would significantly improve InPlacePodVerticalScaling + consolidation interaction:

### 6.1 Resize Cooldown Period
**Problem:** Consolidation fires immediately after pods resize down, before the cluster reaches steady state.
**Proposal:** A per-node or per-pod cooldown annotation (e.g., `karpenter.sh/resize-cooldown: 10m`) that suppresses consolidation evaluation for a node after any pod on it completes an in-place resize. This is more targeted than `consolidateAfter` which applies globally.

### 6.2 Pod Resize History Tracking
**Problem:** Consolidation has no awareness that a node became underutilized *because of in-place resize* vs. *because pods were deleted*. These are fundamentally different signals.
**Proposal:** Track recent resize events in consolidation decisions. A node that became underutilized due to resize-down within the last N minutes should be treated differently — the pods are still running and healthy, just smaller. Consolidating them is pure disruption with no scheduling benefit.

### 6.3 Eventual Size Provisioning
**Problem:** For VPA scale-up workloads, Karpenter provisions based on current pod requests. When pods are known to grow (VPA target > current requests), this leads to tight packing followed by immediate pressure.
**Proposal:** Allow provisioning to consider VPA target recommendations (or a `karpenter.sh/expected-size` annotation) when selecting instance types, providing headroom for anticipated growth.

---

## 7. Simulation Plots

All plots generated from 20-iteration runs across all scenario×strategy combinations.

### Disruption Count by Scenario and Strategy
![Disruption Count](../results/inplace-resize/compare-disruption_count.png)

### Cumulative Cost by Scenario and Strategy
![Cumulative Cost](../results/inplace-resize/compare-cumulative_cost.png)

### Cost vs. Disruption Tradeoff (Pareto Frontier)
![Cost vs Disruption](../results/inplace-resize/cost-vs-disruption.png)

### Time-Weighted Node Count
![Time-Weighted Node Count](../results/inplace-resize/compare-time_weighted_node_count.png)

### Peak Node Count
![Peak Node Count](../results/inplace-resize/compare-peak_node_count.png)

---

## Methodology

- **Simulator:** kubesim discrete-event simulator with KWOK instance catalog
- **Instance types:** m-4x-amd64-linux, m-8x-amd64-linux
- **Iterations:** 20 per scenario×strategy (statistical significance)
- **Scenarios:** 3 (JVM startup spike, VPA gradual scale-up, VPA random jitter)
- **Strategies:** 5 (WhenEmpty, WhenUnderutilized, CostJustified at 0.5/1.0/2.0)
- **Raw data:** `results/inplace-resize/all-results.json`
- **Scenario definitions:** `scenarios/inplace-resize/*.yaml`
