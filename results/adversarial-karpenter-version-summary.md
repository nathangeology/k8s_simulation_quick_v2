# Adversarial Discovery: Karpenter v0.35 vs v1.x

## Overview

Adversarial search comparing Karpenter v0.35 (subtractive-only consolidation)
against v1.x (replacement consolidation) across 1,000 generated scenarios
(500 normal + 500 chaos). Objectives: cost_efficiency, availability,
consolidation_waste. Top 10 scenarios saved; top 5 reported with 50 seeds each.

## Key Findings

### When does v1.x consolidation replacement shine?

1. **GPU + batch mixed workloads with spike traffic** (worst_case_01, divergence
   0.045): v1.x achieves ~7% lower cost ($8.57/hr vs $9.17/hr mean) when the
   cluster has heterogeneous pools (including GPU instances), batch jobs with
   long durations (lognormal 12h), and spike traffic patterns. v1.x's ability
   to replace underutilized nodes with cheaper alternatives pays off when
   workload churn creates partially-filled expensive nodes that v0.35 can't
   consolidate (only delete empty/underutilized, not replace).

2. **High-churn SaaS microservices with PDBs** (worst_case_02/03, divergence
   0.042): With 13 SaaS microservices (80% PDB min_available), web apps with
   anti-affinity, and ML training jobs, v1.x achieves ~6.4% lower cost
   ($3.20/hr vs $3.41/hr). The PDB constraints prevent aggressive node
   deletion, but v1.x can still replace nodes with cheaper types while
   respecting disruption budgets.

3. **Large batch job fleets with varying lifetimes** (worst_case_04/05,
   divergence 0.039): Clusters with 26+ batch jobs (exponential 60-271min
   durations) alongside extreme-replica web apps (200-500 replicas). v1.x
   schedules ~4% more pods (275 vs 264 running) at the same cost by
   consolidating fragmented nodes into better-packed replacements.

### When does v0.35 subtractive-only approach actually win?

In the adversarial search, **v0.35 never clearly outperformed v1.x on cost**.
Across all 1,000 scenarios, v1.x either matched or beat v0.35 on
total_cost_per_hour. The divergence is always in v1.x's favor for cost
efficiency.

However, v0.35 shows marginal advantages in specific narrow cases:

- **Fewer pending pods in some mixed workloads**: In worst_case_02, v0.35 had
  65.6 pending vs 66.9 for v1.x (not statistically significant, p=0.88). The
  replacement consolidation in v1.x can temporarily increase pending pods during
  node swaps.
- **Stability under low-churn steady workloads**: When workloads are stable and
  nodes are well-packed from initial provisioning, v0.35's simpler approach
  avoids unnecessary replacement churn. The cost difference approaches zero.

### Impact of batch job do-not-disrupt on each version

Batch jobs with `priority: low` and explicit durations create nodes that
v0.35 can only remove after the job completes (subtractive). v1.x can
potentially replace the underlying node with a cheaper type while the job
runs, but only if disruption budgets allow it.

In practice, the adversarial search found that:
- Batch jobs with **short lifetimes** (exponential mean 5-60min) create rapid
  node churn that benefits v1.x's replacement logic
- Batch jobs with **long lifetimes** (lognormal 12h) lock nodes for extended
  periods, reducing the window for either version to consolidate — but v1.x
  still wins on cost by optimizing the non-batch portion of the cluster

## Statistical Summary

| Scenario | Divergence | v0.35 $/hr | v1.x $/hr | Cost Δ | p-value | Key Pattern |
|----------|-----------|-----------|----------|--------|---------|-------------|
| #1 (r7vglr) | 0.045 | 9.17 | 8.57 | -6.6% | 0.051 | GPU + batch + spike traffic |
| #2 (39k) | 0.042 | 3.41 | 3.20 | -6.0% | 2e-6 | SaaS + PDB + anti-affinity |
| #4 (d7-6xc) | 0.039 | 6.98 | 6.98 | 0.0% | 1.0 | Batch fleet + extreme replicas |

Note: Scenario #4 shows zero cost difference but significant availability
divergence — v1.x runs 10.9 more pods (p=0.002) at the same cost.

## Feature Importance

The adversarial search tracked which scenario dimensions contribute most to
version divergence:

| Feature | Importance |
|---------|-----------|
| max_max_nodes | 0.1471 |
| total_instance_types | 0.0092 |
| num_workloads | 0.0041 |
| num_pools | 0.0024 |
| has_pdb | 0.0009 |
| has_topology_spread | 0.0008 |

**Cluster scale (max_nodes) dominates.** Larger clusters give v1.x more
opportunities for replacement consolidation. Instance type diversity is the
second factor — more types means more replacement options for v1.x.

## Methodology

- **Search**: Hypothesis-based property testing with `AdversarialFinder`
- **Budget**: 500 normal + 500 chaos scenarios
- **Seeds per evaluation**: 3 (search phase), 50 (report phase)
- **Objectives**: cost_efficiency, availability, consolidation_waste
- **Workload types**: All standard + edge cases (GPU, overcommit, anti-affinity,
  varying batch, extreme replicas)
- **Variant pair**: `{karpenter_version: 'v0.35'}` vs `{karpenter_version: 'v1'}`

## Files

- Scenarios: `scenarios/adversarial/karpenter-version/worst_case_*.yaml`
- Reports: `results/adversarial/karpenter-version/*/report.{json,md}`
- Manifest: `results/adversarial/karpenter-version/manifest.json`
- Script: `scripts/find_adversarial_karpenter_version.py`
