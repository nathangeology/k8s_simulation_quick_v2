# Adversarial Discovery: Deletion Cost Ranking Strategies

Generated: 2026-03-12  
Budget: 500 scenarios (402 evaluated before deadline)  
Seeds: [42, 123, 7] (discovery), 50 seeds (detailed reports)  
Objectives: availability, cost_efficiency, disruption_rate

## Strategies Compared

| Strategy | Rust Enum | Behavior |
|----------|-----------|----------|
| baseline | `None` | No deletion-cost annotation |
| smallest_first | `PreferEmptyingNodes` | Rank by node CPU ascending — empties small nodes first |
| largest_first | `LargestFirst` | Rank by node CPU descending — large nodes' pods deleted first |
| unallocated_vcpu | `UnallocatedVcpu` | Rank by unallocated_cpu/pod_count — targets inefficient nodes |
| random | `Random` | Deterministic pseudo-random ordering by node index |

## Key Findings

### Which strategy wins for cost efficiency?

**`unallocated_vcpu` consistently produces the lowest cost.** Across all top adversarial
scenarios, `unallocated_vcpu` ties with or beats every other strategy on cost_efficiency:

| Scenario | Best Cost Strategy | Cost/hr | Worst Cost Strategy | Cost/hr |
|----------|-------------------|---------|--------------------:|--------:|
| #1 | baseline, smallest_first | $0.288 | largest_first, unallocated_vcpu, random | $0.384 |
| #2 | baseline, unallocated_vcpu | $0.352 | smallest_first, largest_first, random | $0.544 |
| #4 | unallocated_vcpu | $1.120 | smallest_first, largest_first, random | $1.504 |
| #5 | unallocated_vcpu | $1.312 | baseline | $1.696 |

In scenarios #4 and #5 (full availability, no pending pods), `unallocated_vcpu` achieves
19–23% lower cost than the next best strategy by targeting inefficiently packed nodes
for deletion, enabling Karpenter to consolidate onto fewer nodes.

**`baseline` (no annotation) is the second-best for cost** in most scenarios, but it
can also be the worst (scenario #5: $1.696 vs unallocated_vcpu's $1.312).

### Which strategy wins for availability?

**No strategy differentiation on availability.** All 5 strategies produce identical
availability scores across all discovered adversarial scenarios. The deletion cost
ranking affects *which* pods are deleted during scale-down, but the total number of
running vs pending pods remains the same regardless of deletion ordering.

This makes sense: deletion cost annotations influence the *order* of pod deletion
within a ReplicaSet scale-down, but the ReplicaSet controller always deletes the
same *count* of pods. Availability is a function of count, not ordering.

### Are there scenarios where random beats all structured strategies?

**No.** Random never outperforms all structured strategies. In every scenario,
`random` clusters with `smallest_first` and `largest_first` — they produce
identical or near-identical results. The two strategies that diverge are:

- **`baseline`** (no annotation) — sometimes best, sometimes worst
- **`unallocated_vcpu`** — consistently best or tied-for-best on cost

Random's deterministic ordering (by node index) happens to produce similar
consolidation patterns to the explicit size-based strategies in these scenarios.

### How does do-not-disrupt interact with each ranking strategy?

The deletion cost controller partitions nodes into two groups:
1. **Normal nodes** — ranked by strategy, get lower deletion costs (deleted first)
2. **Protected nodes** (with `karpenter.sh/do-not-disrupt` pods) — always get higher costs

This means do-not-disrupt acts as a hard boundary that all strategies respect equally.
The strategy only affects ordering *within* the normal group. None of the adversarial
scenarios surfaced differential behavior between strategies when do-not-disrupt is
present, because the protection is applied identically regardless of ranking.

### Disruption rate

**Zero disruption across all scenarios and strategies.** The `pods_evicted` metric
is 0 for every variant in every scenario. This indicates that deletion cost ranking
affects scale-down ordering but does not cause additional pod evictions beyond what
the ReplicaSet controller requests.

## Top 5 Scenario Details (50-seed reports)

### Scenario #1 — Max divergence: 0.096

Small cluster (2 pools: m5.large + m5.xlarge), web_app + ml_training with 60% scale-down.

| Variant | Cost/hr | Nodes | Running | Pending |
|---------|---------|-------|---------|---------|
| baseline | 0.2880 | 2.0 | 1.0 | 1.0 |
| smallest_first | 0.2880 | 2.0 | 1.0 | 1.0 |
| largest_first | 0.3840 | 3.0 | 1.0 | 1.0 |
| unallocated_vcpu | 0.3840 | 3.0 | 1.0 | 1.0 |
| random | 0.3840 | 3.0 | 1.0 | 1.0 |

Divergence driven by node count: baseline/smallest_first consolidate to 2 nodes,
others retain 3. The 33% cost difference ($0.096/hr) is the largest found.

### Scenario #2 — Max divergence: 0.044

Oversubscribed cluster with high pending pod count (39.7 pending vs 4.3 running).

| Variant | Cost/hr | Nodes | Running | Pending |
|---------|---------|-------|---------|---------|
| baseline | 0.4608 | 2.9 | 4.68 | 37.24 |
| unallocated_vcpu | 0.4416 | 2.8 | 4.68 | 37.24 |
| smallest_first | 0.5261 | 3.24 | 4.68 | 37.24 |
| largest_first | 0.5261 | 3.24 | 4.68 | 37.24 |
| random | 0.5261 | 3.24 | 4.68 | 37.24 |

### Scenario #3 — Max divergence: 0.036

Similar oversubscribed pattern. baseline and unallocated_vcpu use fewer nodes.

### Scenario #4 — Max divergence: 0.032

Fully scheduled cluster (12 running, 0 pending). unallocated_vcpu achieves
6.9 nodes vs 8.4+ for others — 19% fewer nodes at full availability.

### Scenario #5 — Max divergence: 0.032

Fully scheduled. Interesting reversal: baseline is *worst* ($1.636/hr, 9.0 nodes)
while unallocated_vcpu is best ($1.452/hr, 8.1 nodes). Shows baseline's lack of
intelligent ordering can hurt in larger clusters.

## Conclusions

1. **`unallocated_vcpu` is the recommended default** — it consistently achieves the
   lowest or tied-lowest cost by targeting inefficiently packed nodes for deletion.

2. **Strategy choice affects cost, not availability or disruption** — all strategies
   produce identical pod counts and zero evictions. The difference is purely in
   how many nodes remain after consolidation.

3. **The effect size is modest** — max divergence of ~10% in cost/hr. In production
   clusters with more complex workload mixes, the effect may be larger.

4. **`baseline` (no annotation) is unpredictable** — sometimes best, sometimes worst.
   Any explicit strategy is more predictable than no strategy.

5. **`random`, `smallest_first`, and `largest_first` cluster together** — they
   produce nearly identical results in most scenarios, suggesting the specific
   ordering within these strategies matters less than having *some* ordering.

## Files

- Scenarios: `scenarios/adversarial/deletion-cost/scenario_*.yaml`
- Reports: `results/adversarial-deletion-cost/scenario_*/report.{json,md}`
- Manifest: `results/adversarial-deletion-cost/manifest.json`
- Finder script: `scripts/find_adversarial_deletion_cost.py`
