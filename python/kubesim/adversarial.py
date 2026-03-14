"""Adversarial scenario finder for KubeSim (Mode 4).

Coverage-guided search over the scenario space to find worst-case and best-case
scenarios for a given metric.  Uses ``batch_run`` for fast parallel evaluation
and optionally leverages Hypothesis shrinking to produce minimal adversarial
examples.

Supports multi-objective search, chaos mode, feature importance tracking,
and arbitrary variant pair comparison.

Usage::

    from kubesim.adversarial import AdversarialFinder, ScenarioSpace

    finder = AdversarialFinder(
        objective="maximize",
        metric=lambda results: results[0]["total_cost_per_hour"],
        space=ScenarioSpace(nodes=(10, 200)),
        budget=500,
    )
    ranked = finder.run()

    # Multi-objective search
    from kubesim.objectives import cost_efficiency, availability
    finder = AdversarialFinder(
        objective="maximize",
        metric=cost_efficiency,
        objectives=[cost_efficiency, availability],
        space=ScenarioSpace(nodes=(10, 200)),
        budget=500,
    )
    ranked = finder.run()
"""

from __future__ import annotations

import random
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Callable, Literal

import yaml

from kubesim.strategies import cluster_scenario, chaos_scenario


@dataclass
class ScenarioSpace:
    """Defines the search bounds for adversarial scenario generation."""

    nodes: tuple[int, int] = (10, 200)
    workload_types: list[str] | None = None
    min_workloads: int = 1
    max_workloads: int = 8
    traffic: bool | None = None
    min_pools: int = 1
    max_pools: int = 3


@dataclass
class ScoredScenario:
    """A scenario together with its evaluation score."""

    scenario: dict
    score: float
    seed: int = 0
    objective_scores: tuple[float, ...] = ()
    feature_contributions: dict[str, float] = field(default_factory=dict)


@dataclass
class VariantPair:
    """A pair of variants to compare."""

    name_a: str
    config_a: dict
    name_b: str
    config_b: dict


# Default variant pairs
MOST_VS_LEAST = VariantPair(
    name_a="most_allocated",
    config_a={"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    name_b="least_allocated",
    config_b={"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
)

KARPENTER_CONSOLIDATION = VariantPair(
    name_a="when_empty",
    config_a={"name": "when_empty", "karpenter": {"consolidation": {"policy": "WhenEmpty"}}},
    name_b="when_underutilized",
    config_b={"name": "when_underutilized", "karpenter": {"consolidation": {"policy": "WhenUnderutilized"}}},
)

DELETION_COST_PAIRS = VariantPair(
    name_a="no_deletion_cost",
    config_a={"name": "no_deletion_cost", "deletion_cost_strategy": "none"},
    name_b="cost_aware",
    config_b={"name": "cost_aware", "deletion_cost_strategy": "cost_aware"},
)


def _extract_features(scenario: dict) -> dict[str, float]:
    """Extract numeric features from a scenario for importance tracking."""
    study = scenario.get("study", scenario)
    features: dict[str, float] = {}
    pools = study.get("cluster", {}).get("node_pools", [])
    features["num_pools"] = len(pools)
    features["max_max_nodes"] = max((p.get("max_nodes", 0) for p in pools), default=0)
    features["total_instance_types"] = sum(len(p.get("instance_types", [])) for p in pools)
    features["single_instance_pool"] = float(any(len(p.get("instance_types", [])) == 1 for p in pools))
    wls = study.get("workloads", [])
    features["num_workloads"] = len(wls)
    features["has_traffic"] = float("traffic_pattern" in study)
    features["has_gpu_workload"] = float(any("gpu_request" in w for w in wls))
    features["has_pdb"] = float(any("pdb" in w for w in wls))
    features["has_topology_spread"] = float(any("topology_spread" in w for w in wls))
    features["has_anti_affinity"] = float(any("pod_anti_affinity" in w for w in wls))
    return features


@dataclass
class AdversarialFinder:
    """Search for extreme scenarios under a user-defined metric.

    Args:
        objective: ``"maximize"`` or ``"minimize"`` the metric.
        metric: Callable receiving the list of result dicts and returning a float.
        objectives: Optional list of objective functions for multi-objective search.
        space: :class:`ScenarioSpace` bounding the search.
        budget: Maximum number of scenario evaluations.
        seeds: Seeds passed to ``batch_run`` per evaluation.
        top_k: Number of extreme scenarios to keep.
        seed: RNG seed for reproducibility.
        chaos: Enable chaos mode (combines worst-case configs).
        variant_pair: Variant pair to compare. Defaults to MostAllocated vs LeastAllocated.
        track_features: Track which config dimensions contribute to divergence.
    """

    objective: Literal["maximize", "minimize"]
    metric: Callable[[list[dict]], float]
    objectives: list[Callable[[list[dict]], float]] = field(default_factory=list)
    space: ScenarioSpace = field(default_factory=ScenarioSpace)
    budget: int = 1000
    seeds: list[int] = field(default_factory=lambda: [42])
    top_k: int = 10
    seed: int = 0
    chaos: bool = False
    variant_pair: VariantPair | None = None
    track_features: bool = False
    screen_threshold: float = 0.01

    # ── public API ───────────────────────────────────────────────

    def run(self) -> list[ScoredScenario]:
        """Execute the search and return ranked extreme scenarios."""
        from hypothesis import given, settings, HealthCheck

        rng = random.Random(self.seed)
        strat = self._build_strategy()

        found: list[ScoredScenario] = []
        counter = {"n": 0}
        feature_scores: dict[str, list[float]] = defaultdict(list) if self.track_features else {}

        @settings(
            max_examples=self.budget,
            database=None,
            suppress_health_check=[HealthCheck.too_slow],
            derandomize=True,
            deadline=None,
        )
        @given(scenario=strat)
        def _search(scenario: dict) -> None:
            if counter["n"] >= self.budget:
                return
            counter["n"] += 1
            score = self._evaluate(scenario, rng)
            obj_scores = tuple(obj(self._last_results) for obj in self.objectives) if self.objectives else ()
            feat_contrib = {}
            if self.track_features:
                feat_contrib = _extract_features(scenario)
                for k, v in feat_contrib.items():
                    feature_scores[k].append(v * abs(score))
            found.append(ScoredScenario(
                scenario=scenario, score=score, seed=self.seed,
                objective_scores=obj_scores, feature_contributions=feat_contrib,
            ))

        self._last_results: list[dict] = []
        try:
            _search()
        except AssertionError:
            pass

        reverse = self.objective == "maximize"
        found.sort(key=lambda s: s.score, reverse=reverse)
        result = found[:self.top_k]

        if self.track_features and feature_scores:
            self.feature_importance = {
                k: sum(v) / len(v) for k, v in feature_scores.items()
            }

        return result

    def shrink(self, scenario: dict) -> ScoredScenario:
        """Use Hypothesis shrinking to find a minimal adversarial example."""
        from hypothesis import given, settings, HealthCheck

        baseline = self._evaluate(scenario, random.Random(self.seed))
        best = ScoredScenario(scenario=scenario, score=baseline, seed=self.seed)
        is_max = self.objective == "maximize"
        strat = self._build_strategy()

        @settings(
            max_examples=200,
            database=None,
            suppress_health_check=[HealthCheck.too_slow],
            derandomize=True,
            deadline=None,
        )
        @given(candidate=strat)
        def _shrink(candidate: dict) -> None:
            nonlocal best
            score = self._evaluate(candidate, random.Random(self.seed))
            if (is_max and score >= baseline) or (not is_max and score <= baseline):
                if (is_max and score >= best.score) or (not is_max and score <= best.score):
                    best = ScoredScenario(scenario=candidate, score=score, seed=self.seed)
                assert False  # noqa: B011

        try:
            _shrink()
        except AssertionError:
            pass

        return best

    # ── internals ────────────────────────────────────────────────

    def _build_strategy(self):
        """Build the Hypothesis strategy based on config."""
        if self.chaos:
            return chaos_scenario(max_nodes=self.space.nodes[1])
        return cluster_scenario(
            max_nodes=self.space.nodes[1],
            workload_types=self.space.workload_types,
            min_workloads=self.space.min_workloads,
            max_workloads=self.space.max_workloads,
            traffic=self.space.traffic,
            min_pools=self.space.min_pools,
            max_pools=self.space.max_pools,
        )

    def _evaluate(self, scenario: dict, rng: random.Random) -> float:
        """Serialize scenario to YAML, run via batch_run, return metric score.

        Uses two-phase progressive evaluation: a quick screen with 1 seed,
        then full evaluation only if the quick score meets the threshold.
        """
        from kubesim._native import batch_run

        study = scenario.get("study", scenario)

        # Inject variant pair if specified
        if self.variant_pair:
            study["variants"] = [self.variant_pair.config_a, self.variant_pair.config_b]
        elif not study.get("variants"):
            study["variants"] = [{"name": "baseline", "scheduler": {"scoring": "LeastAllocated", "weight": 1}}]

        # Use faster scheduling path during search
        study["scheduling_strategy"] = "reverse_schedule"

        config_yaml = yaml.dump(scenario, default_flow_style=False)

        # Phase 1: Quick screen with 1 seed
        quick_raw = batch_run(config_yaml, [self.seeds[0]])
        quick_results = [dict(r) if not isinstance(r, dict) else r for r in quick_raw]
        quick_score = float(self.metric(quick_results))
        if abs(quick_score) < self.screen_threshold:
            self._last_results = quick_results
            return quick_score

        # Phase 2: Full evaluation with all seeds
        raw = batch_run(config_yaml, self.seeds)
        self._last_results = [dict(r) if not isinstance(r, dict) else r for r in raw]
        return float(self.metric(self._last_results))
