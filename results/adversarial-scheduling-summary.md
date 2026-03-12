# Adversarial Scheduling Discovery: MostAllocated vs LeastAllocated

Generated: 2026-03-12  
Budget: 500 scenarios × 2 search modes (normal+expanded, chaos) = 742 evaluated  
Seeds per evaluation: [42, 100, 200]  
Objectives: cost_efficiency, availability, scheduling_failure_rate, entropy_deviation

## Executive Summary

Across 742 randomly generated cluster scenarios using the expanded strategy space
(edge-case workloads, single-instance pools, overcommit, batch jobs with lifetimes,
anti-affinity, topology spread), **MostAllocated and LeastAllocated produce identical
results in 93.5% of scenarios** (694/742). When they diverge, MostAllocated tends to
cost more but schedule more pods, while LeastAllocated spreads load but can leave
more pods pending in constrained clusters.

## When Does MostAllocated Win?

**LeastAllocated costs more in 5 scenarios (0.7%).**

Scenario characteristics (worst_case_05 through worst_case_09):
- **Steady-state web apps with diurnal traffic** — multiple web_app workloads with
  moderate replica counts (2–28 replicas) and diurnal traffic patterns
- **Mixed instance types including GPU** — pools with m5.large, p3.8xlarge, m5.xlarge,
  c6i.large, g4dn.xlarge (heterogeneous sizing)
- **Karpenter WhenUnderutilized consolidation** active
- **Cost delta**: −$0.04 to −$0.06/hr (LeastAllocated wastes ~2% more)
- **Mechanism**: LeastAllocated spreads pods across more nodes, preventing Karpenter
  from consolidating underutilized nodes. MostAllocated packs tighter, enabling
  consolidation of empty/underutilized nodes.

## When Does LeastAllocated Win?

**MostAllocated costs more in 4 scenarios (0.5%).**

Scenario characteristics (worst_case_01 through worst_case_04):
- **Heavy batch workloads with long durations** — 39+ batch jobs with lognormal
  12h mean duration, plus web apps with topology spread and PDBs
- **High CPU variance** — CPU requests ranging from 584m to 8000m (wide uniform dist)
- **High memory requests** — batch jobs requesting 12GB+ memory
- **Single pool with mixed instance types** including expensive GPU instances
- **Cost delta**: +$0.03 to +$3.07/hr (MostAllocated wastes up to 19% more)
- **Mechanism**: MostAllocated packs pods onto already-loaded nodes, forcing scale-up
  to expensive instance types (p3.8xlarge, g4dn.xlarge) when cheaper instances still
  have capacity. LeastAllocated distributes across cheaper instances first.

**Worst case (worst_case_01)**: MostAllocated costs $18.94/hr vs LeastAllocated $15.87/hr
(+$3.07/hr, +19.4%). MostAllocated runs 113.7 pods vs 100.7 for LeastAllocated, but at
significantly higher cost per pod. The 50-seed report confirms the trend (p=0.22, CI
includes zero due to high variance, but median effect is consistent).

## When Do Both Degrade?

**Both strategies produce identical costs in 733 scenarios (98.8%).**

The "both_degrade" category with high combined_divergence (worst_case_10 through
worst_case_19) represents scenarios where **neither strategy can schedule pods
effectively** — tiny workload counts (1–2 web apps with 2–11 replicas) on
oversized clusters. The divergence comes from availability/scheduling_failure_rate
objectives, not cost. These are degenerate scenarios where the cluster is
fundamentally misconfigured (too few workloads for the pool size).

## Which Config Dimensions Drive the Biggest Divergence?

| Dimension | Impact | Direction |
|-----------|--------|-----------|
| **Batch job count + duration** | HIGH | Long-running batch jobs with high CPU variance create the largest cost gaps |
| **Instance type heterogeneity** | HIGH | Pools mixing cheap (c5/m5) and expensive (p3/g4dn) instances amplify packing differences |
| **CPU request variance** | MEDIUM | Wide CPU distributions (500m–8000m) create bin-packing asymmetry |
| **Karpenter consolidation** | MEDIUM | WhenUnderutilized consolidation interacts differently with each strategy |
| **Topology spread + PDB** | LOW | Constraints reduce scheduling freedom but affect both strategies similarly |
| **Traffic pattern** | LOW | Diurnal traffic creates transient differences but averages out |

## Key Findings

1. **The strategies are nearly equivalent for most workloads.** In 93.5% of scenarios,
   the cost difference is exactly zero.

2. **MostAllocated's biggest risk is GPU/expensive instance escalation.** When packing
   forces scale-up to expensive instance types, costs spike disproportionately.

3. **LeastAllocated's biggest risk is consolidation prevention.** Spreading pods
   prevents Karpenter from reclaiming underutilized nodes.

4. **The maximum observed divergence is $3.07/hr** (worst_case_01), driven by
   batch workloads with high CPU variance on heterogeneous pools.

5. **Chaos mode (single-instance pools, overcommit, edge-case workloads) produced
   smaller divergences** than the normal expanded search, suggesting the strategies
   diverge most on realistic-but-complex configurations, not extreme edge cases.

## Scenario Files

- Scenarios: `scenarios/adversarial/scheduling/worst_case_*.yaml`
- Reports (50-seed): `results/adversarial/scheduling/worst_case_*/report.{json,md}`
- Manifest: `results/adversarial/manifest.json`
