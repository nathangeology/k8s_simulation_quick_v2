# Adversarial Scoring Proposal: Scale-Invariant Divergence

**Date:** 2026-03-19
**Bead:** k8s-wmo5

---

## 1. Problem Statement

The adversarial search scoring (`_combined_divergence` in `find_adversarial.py`)
sums raw absolute deltas between variant metrics:

```python
total += abs(fn(group_a) - fn(group_b))
```

This creates two biases:

1. **Large-cluster dominance.** `cost_efficiency` returns `total_cost / running_pods`.
   A 200-node cluster might produce `cost_efficiency` values of 3.0–5.0, while a
   10-node cluster produces 0.3–0.5. The absolute delta for the large cluster is
   ~10× larger, so it dominates `combined_divergence` regardless of whether the
   *relative* divergence is meaningful.

2. **Objective scale mismatch.** Three of the four objectives (`availability`,
   `scheduling_failure_rate`, `entropy_deviation`) are bounded ratios in [0, 1].
   `cost_efficiency` is unbounded. Summing them treats a 0.01 cost delta the same
   as a 0.01 availability delta, but their practical significance differs by
   orders of magnitude.

A naive percent-based fix would flip the bias: small clusters have quantized
metrics (e.g., 2 vs 3 nodes = 50% difference in cost_efficiency) that would
dominate instead.

### Current Scoring Flow

```
_combined_divergence(results)
  → split by variant
  → for each of 4 objectives: abs(fn(variant_a) - fn(variant_b))
  → sum all deltas
  → used as Optuna objective (maximize)
```

The same raw-delta approach appears in `_categorize` (for ranking) and
`OptunaAdversarialSearch` (as the optimization target).

---

## 2. Analysis: Score Distribution vs Cluster Size

### Theoretical Analysis

Given the objective function definitions:

| Objective | Formula | Range | Scales with cluster size? |
|-----------|---------|-------|--------------------------|
| `cost_efficiency` | total_cost / running_pods | [0, ∞) | Yes — cost grows with nodes |
| `availability` | running / (running + pending) | [0, 1] | No — ratio |
| `scheduling_failure_rate` | pending / (running + pending) | [0, 1] | No — ratio |
| `entropy_deviation` | \|avg_entropy - 1.0\| | [0, 1] | No — ratio |

In `_combined_divergence`, the cost_efficiency delta can easily be 0.5–2.0 for
large clusters, while the three ratio-based objectives produce deltas of 0.0–0.1
in typical scenarios. This means **cost_efficiency contributes 80–95% of the
combined score** for large clusters, effectively making the search a
single-objective cost optimizer.

### Empirical Evidence from Existing Scenarios

The generated adversarial scenarios confirm this pattern:
- `scheduling/worst_case_01.yaml`: max_nodes=68, complex workloads → high combined_divergence
- `scheduling/worst_case_13.yaml`: max_nodes=11, simple workloads → low combined_divergence
- The top-ranked scenarios consistently have larger `max_nodes` values

The audit document (adversarial-search-audit.md §1.4) notes that Hypothesis
shrinking converges to minimal configs — but when it doesn't shrink (Optuna
search), large clusters dominate the top-k.

---

## 3. Candidate Scoring Functions

Three scale-invariant alternatives are proposed, implemented in
`python/kubesim/scale_invariant_scoring.py`.

### Candidate 1: Symmetric Log-Ratio

```python
score = Σ |ln(|a| / |b|)|   for each objective
```

**Properties:**
- Perfectly scale-invariant: `ln(2a / 2b) = ln(a / b)`
- Symmetric: `|ln(a/b)| = |ln(b/a)|`
- Handles the ratio objectives naturally (already scale-free)
- Requires clamping near zero to avoid `ln(0)`

**Strengths:**
- Mathematically clean scale invariance
- No tuning parameters beyond the epsilon floor
- Well-understood in statistics (log-ratio is the standard scale-free comparison)

**Weaknesses:**
- Undefined when one value is zero and the other isn't (clamped, but the clamp
  value affects scoring near zero)
- Treats a 2× difference the same regardless of whether it's 0.001 vs 0.002
  or 1.0 vs 2.0 — may over-weight tiny absolute differences

**Best for:** Scenarios where the *relative* magnitude of divergence matters
more than the absolute magnitude. Good default choice.

### Candidate 2: Rank-Normalized (Z-Score) Divergence

```python
scorer = RankNormalizedScorer(objective_names)
# During search:
scorer.observe(deltas)  # updates running mean/variance
score = Σ |z_i|         # z_i = (delta_i - mean_i) / std_i
```

**Properties:**
- Adapts to the actual distribution of each objective across the search
- After warm-up, each objective contributes equally in standard-deviation units
- Automatically handles different scales without manual normalization

**Strengths:**
- No assumptions about objective ranges — learns from data
- Naturally balances objectives by their observed variability
- A scenario that's 2σ above mean on availability divergence scores the same
  as one that's 2σ above mean on cost divergence

**Weaknesses:**
- Requires warm-up period (~20 observations) before scores are meaningful
- Scores are relative to the population — not comparable across different search runs
- Stateful: the scorer must persist across the search loop
- Early observations during warm-up use raw deltas (fallback)

**Best for:** Long search runs (budget ≥ 100) where population statistics
stabilize. Ideal for Optuna TPE which benefits from consistent scoring.

### Candidate 3: Clamped-Relative Divergence

```python
score = Σ |a - b| / max(|a|, |b|, floor)   for each objective
```

**Properties:**
- Each term bounded in [0, 1] (when floor is inactive)
- The floor parameter prevents small-value quantization noise
- Simple, stateless, no warm-up needed

**Strengths:**
- Intuitive: "what fraction of the larger value is the difference?"
- The floor parameter directly addresses the small-cluster quantization problem
- Stateless — works identically for trial 1 and trial 1000
- Each objective naturally bounded, so they contribute comparably

**Weaknesses:**
- The floor parameter requires tuning (proposed default: 0.01)
- Not perfectly scale-invariant when the floor is active
- For objectives already in [0, 1], the floor may be too aggressive
  (e.g., availability of 0.005 vs 0.010 = 50% relative diff, but floor
  clamps denominator to 0.01, yielding 0.5 instead of 1.0)

**Best for:** Simple drop-in replacement with minimal code changes. Good when
you want predictable, bounded scores without statefulness.

---

## 4. Comparison Matrix

| Property | Raw (current) | Log-Ratio | Z-Score | Clamped-Relative |
|----------|:---:|:---:|:---:|:---:|
| Scale-invariant | ✗ | ✓ | ✓ | ~✓ |
| Stateless | ✓ | ✓ | ✗ | ✓ |
| No warm-up | ✓ | ✓ | ✗ | ✓ |
| Bounded per-objective | ✗ | ✗ | ✗ | ✓ |
| Handles zero values | ✓ | ~✓ (clamp) | ✓ | ✓ |
| No tuning params | ✓ | ✓ | ~✓ (min_obs) | ✗ (floor) |
| Cross-run comparable | ✓ | ✓ | ✗ | ✓ |
| Balances objectives | ✗ | ~✓ | ✓ | ✓ |

---

## 5. Recommendation

**Primary: Clamped-Relative Divergence** (Candidate 3)

Rationale:
1. **Simplest integration.** Drop-in replacement for `_combined_divergence` with
   no state management. The existing `_categorize`, Optuna objective, and ranking
   code all work unchanged.
2. **Bounded and balanced.** Each objective contributes at most 1.0 to the total,
   so no single objective can dominate. The four-objective sum ranges from 0 to 4.
3. **Floor handles quantization.** The `abs_floor=0.01` prevents small-cluster
   noise without requiring population statistics.
4. **Predictable.** Scores are deterministic and comparable across search runs,
   which matters for reproducibility and for comparing results across sessions.

**Secondary: Log-Ratio** as a validation cross-check. Run both scorers on the
same search and compare top-k overlap. If they agree on 70%+ of top-k scenarios,
the results are robust to scoring choice.

**Z-Score** is best suited for a future multi-phase search where Phase 1 builds
population statistics and Phase 2 uses them for refined scoring. Not recommended
as the primary scorer due to statefulness and warm-up requirements.

---

## 6. Integration Plan

### Step 1: Add scoring module (this PR)

New file: `python/kubesim/scale_invariant_scoring.py`
- `log_ratio_divergence(results, objective_fns) -> float`
- `RankNormalizedScorer` class + `rank_normalized_divergence()`
- `clamped_relative_divergence(results, objective_fns, abs_floor) -> float`

### Step 2: Replace `_combined_divergence` in `find_adversarial.py`

```python
# Before:
def _combined_divergence(results):
    ...
    total += abs(a - b)  # raw delta

# After:
from kubesim.scale_invariant_scoring import clamped_relative_divergence

def _combined_divergence(results):
    return clamped_relative_divergence(results, OBJECTIVE_FNS)
```

### Step 3: Update `_categorize` to use relative scoring

The `_categorize` function should report both raw and relative metrics:
- Keep `signed_delta` and `abs_delta` for cost (useful for human interpretation)
- Replace `combined_divergence` with the clamped-relative score
- Add `combined_divergence_raw` for backward compatibility if needed

### Step 4: Validate with adversarial search run

Re-run `find_adversarial.py` with the new scoring and compare:
- Top-k scenario distribution by cluster size (should be more uniform)
- Diversity of top-k scenarios (should increase)
- Whether known adversarial patterns (from scenario_templates.py) still rank highly

---

## 7. Future Work

- **Per-objective weighting.** Allow users to specify relative importance of
  objectives (e.g., 2× weight on availability vs cost). The clamped-relative
  scorer supports this trivially by multiplying each term.
- **Adaptive floor.** Instead of a fixed `abs_floor`, compute it as a percentile
  of observed values (e.g., 10th percentile). This would make the floor
  self-tuning across different cluster configurations.
- **Multi-objective Pareto scoring.** Replace the scalar sum with Pareto
  dominance counting — a scenario scores higher if it's non-dominated on more
  objective pairs. This avoids the need to combine objectives at all.
