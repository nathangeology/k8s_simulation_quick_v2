"""Scale-invariant divergence scoring for adversarial scenario ranking.

Replaces raw absolute-delta scoring (``_combined_divergence``) with measures
that are invariant to cluster size, preventing large clusters from dominating
the top-k results.

Three candidate scoring functions are provided:

1. **Log-ratio divergence** — symmetric log-ratio of each objective, averaged.
2. **Rank-normalized divergence** — z-score each objective delta against a
   running population, then sum the normalized deltas.
3. **Clamped-relative divergence** — relative difference with a minimum
   absolute floor to avoid small-cluster quantization noise.

Usage::

    from kubesim.scale_invariant_scoring import (
        log_ratio_divergence,
        rank_normalized_divergence,
        clamped_relative_divergence,
    )

    # Drop-in replacement for _combined_divergence in find_adversarial.py
    score = log_ratio_divergence(results, OBJECTIVE_FNS)
"""

from __future__ import annotations

import math
from typing import Callable

ObjectiveFn = Callable[[list[dict]], float]

# Floor to avoid division-by-zero and dampen quantization noise on tiny values.
_EPSILON = 1e-6
# Minimum absolute value for the denominator in relative scoring.
# Objectives returning values below this are treated as effectively zero.
_ABS_FLOOR = 0.01


def _split_variants(results: list[dict]) -> tuple[list[dict], list[dict]] | None:
    """Split results into two variant groups. Returns None if < 2 variants."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)
    if len(by_variant) < 2:
        return None
    groups = list(by_variant.values())
    return groups[0], groups[1]


# ── Candidate 1: Symmetric log-ratio ────────────────────────────


def log_ratio_divergence(
    results: list[dict],
    objective_fns: dict[str, ObjectiveFn],
) -> float:
    """Symmetric log-ratio divergence across objectives.

    For each objective, computes ``|ln(a / b)|`` (symmetric in a, b).
    This is scale-invariant: doubling both a and b yields the same score.
    Objectives where both values are near zero are skipped.

    Returns the sum of log-ratios across all objectives.
    """
    pair = _split_variants(results)
    if pair is None:
        return 0.0
    group_a, group_b = pair

    total = 0.0
    for fn in objective_fns.values():
        a, b = fn(group_a), fn(group_b)
        # Skip if both near zero or either is infinite
        if not (math.isfinite(a) and math.isfinite(b)):
            continue
        a_abs, b_abs = abs(a), abs(b)
        if a_abs < _EPSILON and b_abs < _EPSILON:
            continue
        # Clamp to epsilon to avoid log(0)
        a_safe = max(a_abs, _EPSILON)
        b_safe = max(b_abs, _EPSILON)
        total += abs(math.log(a_safe / b_safe))
    return total


# ── Candidate 2: Rank-normalized (z-score) divergence ───────────


class RankNormalizedScorer:
    """Accumulates objective deltas and scores new observations as z-scores.

    Maintains a running mean and variance per objective. Each new observation's
    delta is converted to a z-score, and the final divergence is the sum of
    absolute z-scores. This automatically adapts to the scale of each objective
    across the population of evaluated scenarios.

    Must call ``observe`` for each scenario before ``score`` is meaningful.
    After a warm-up period (``min_observations``), scores reflect how unusual
    a scenario's divergence is relative to the population.
    """

    def __init__(self, objective_names: list[str], min_observations: int = 20):
        self._names = objective_names
        self._min_obs = min_observations
        # Welford's online algorithm state per objective
        self._n: int = 0
        self._mean: dict[str, float] = {k: 0.0 for k in objective_names}
        self._m2: dict[str, float] = {k: 0.0 for k in objective_names}

    def observe(self, deltas: dict[str, float]) -> None:
        """Record a new set of per-objective absolute deltas."""
        self._n += 1
        for k in self._names:
            d = abs(deltas.get(k, 0.0))
            old_mean = self._mean[k]
            self._mean[k] += (d - old_mean) / self._n
            self._m2[k] += (d - old_mean) * (d - self._mean[k])

    def score(self, deltas: dict[str, float]) -> float:
        """Return sum of absolute z-scores for the given deltas.

        During warm-up (< min_observations), falls back to sum of raw deltas
        so early trials still get a nonzero score.
        """
        if self._n < self._min_obs:
            return sum(abs(deltas.get(k, 0.0)) for k in self._names)
        total = 0.0
        for k in self._names:
            d = abs(deltas.get(k, 0.0))
            var = self._m2[k] / self._n if self._n > 0 else 0.0
            std = math.sqrt(var) if var > 0 else _EPSILON
            total += abs((d - self._mean[k]) / std)
        return total


def rank_normalized_divergence(
    results: list[dict],
    objective_fns: dict[str, ObjectiveFn],
    scorer: RankNormalizedScorer,
) -> float:
    """Z-score normalized divergence using a population-aware scorer.

    Computes per-objective deltas, observes them into the scorer's running
    statistics, and returns the z-score-based divergence.
    """
    pair = _split_variants(results)
    if pair is None:
        return 0.0
    group_a, group_b = pair

    deltas: dict[str, float] = {}
    for name, fn in objective_fns.items():
        a, b = fn(group_a), fn(group_b)
        d = a - b if (math.isfinite(a) and math.isfinite(b)) else 0.0
        deltas[name] = d

    scorer.observe(deltas)
    return scorer.score(deltas)


# ── Candidate 3: Clamped-relative divergence ────────────────────


def clamped_relative_divergence(
    results: list[dict],
    objective_fns: dict[str, ObjectiveFn],
    abs_floor: float = _ABS_FLOOR,
) -> float:
    """Relative divergence with a minimum absolute floor per objective.

    For each objective, computes::

        |a - b| / max(|a|, |b|, abs_floor)

    The floor prevents small-cluster quantization noise (e.g. 2 vs 3 nodes =
    50% relative diff) from dominating. Each term is bounded in [0, 1] when
    the floor is not active, making objectives comparable without explicit
    normalization.

    Returns the sum of clamped-relative divergences.
    """
    pair = _split_variants(results)
    if pair is None:
        return 0.0
    group_a, group_b = pair

    total = 0.0
    for fn in objective_fns.values():
        a, b = fn(group_a), fn(group_b)
        if not (math.isfinite(a) and math.isfinite(b)):
            continue
        delta = abs(a - b)
        denom = max(abs(a), abs(b), abs_floor)
        total += delta / denom
    return total
