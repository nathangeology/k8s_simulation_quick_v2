"""Adversarial scenario finder for KubeSim (Mode 4).

Coverage-guided search over the scenario space to find worst-case and best-case
scenarios for a given metric.  Uses ``batch_run`` for fast parallel evaluation
and optionally leverages Hypothesis shrinking to produce minimal adversarial
examples.

Usage::

    from kubesim.adversarial import AdversarialFinder, ScenarioSpace

    finder = AdversarialFinder(
        objective="maximize",
        metric=lambda results: results[0]["total_cost_per_hour"],
        space=ScenarioSpace(nodes=(10, 200), workload_types=["web_app", "batch_job"]),
        budget=500,
    )
    ranked = finder.run()
    # ranked[0] is the most extreme scenario found
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from typing import Callable, Literal

import yaml

from kubesim.strategies import cluster_scenario


@dataclass
class ScenarioSpace:
    """Defines the search bounds for adversarial scenario generation.

    Each parameter constrains the corresponding ``cluster_scenario`` strategy.
    ``None`` means "use the strategy default".
    """

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


@dataclass
class AdversarialFinder:
    """Search for extreme scenarios under a user-defined metric.

    Args:
        objective: ``"maximize"`` or ``"minimize"`` the metric.
        metric: Callable receiving the list of result dicts from a single
            ``batch_run`` evaluation and returning a float score.
        space: :class:`ScenarioSpace` bounding the search.
        budget: Maximum number of scenario evaluations.
        seeds: Seeds passed to ``batch_run`` per evaluation.  Defaults to
            ``[42]`` (single seed for speed).
        top_k: Number of extreme scenarios to keep in the ranked output.
        seed: RNG seed for reproducibility of the search itself.
    """

    objective: Literal["maximize", "minimize"]
    metric: Callable[[list[dict]], float]
    space: ScenarioSpace = field(default_factory=ScenarioSpace)
    budget: int = 1000
    seeds: list[int] = field(default_factory=lambda: [42])
    top_k: int = 10
    seed: int = 0

    # ── public API ───────────────────────────────────────────────

    def run(self) -> list[ScoredScenario]:
        """Execute the search and return ranked extreme scenarios."""
        from hypothesis import given, settings, HealthCheck
        from hypothesis import strategies as st

        results: list[ScoredScenario] = []
        rng = random.Random(self.seed)

        strat = cluster_scenario(
            max_nodes=self.space.nodes[1],
            workload_types=self.space.workload_types,
            min_workloads=self.space.min_workloads,
            max_workloads=self.space.max_workloads,
            traffic=self.space.traffic,
            min_pools=self.space.min_pools,
            max_pools=self.space.max_pools,
        )

        # Use Hypothesis to draw and optionally shrink scenarios
        found: list[ScoredScenario] = []
        counter = {"n": 0}

        @settings(
            max_examples=self.budget,
            database=None,
            suppress_health_check=[HealthCheck.too_slow],
            derandomize=True,
        )
        @given(scenario=strat)
        def _search(scenario: dict) -> None:
            if counter["n"] >= self.budget:
                return
            counter["n"] += 1
            score = self._evaluate(scenario, rng)
            found.append(ScoredScenario(scenario=scenario, score=score, seed=self.seed))

        try:
            _search()
        except AssertionError:
            pass  # Hypothesis may raise when shrinking; we collect results regardless

        reverse = self.objective == "maximize"
        found.sort(key=lambda s: s.score, reverse=reverse)
        return found[: self.top_k]

    def shrink(self, scenario: dict) -> ScoredScenario:
        """Use Hypothesis shrinking to find a minimal adversarial example.

        Starts from *scenario* and attempts to simplify it while keeping the
        metric at least as extreme.
        """
        from hypothesis import given, settings, HealthCheck, reject
        from hypothesis import strategies as st

        baseline = self._evaluate(scenario, random.Random(self.seed))
        best = ScoredScenario(scenario=scenario, score=baseline, seed=self.seed)
        is_max = self.objective == "maximize"

        strat = cluster_scenario(
            max_nodes=self.space.nodes[1],
            workload_types=self.space.workload_types,
            min_workloads=self.space.min_workloads,
            max_workloads=self.space.max_workloads,
            traffic=self.space.traffic,
            min_pools=self.space.min_pools,
            max_pools=self.space.max_pools,
        )

        @settings(
            max_examples=200,
            database=None,
            suppress_health_check=[HealthCheck.too_slow],
            derandomize=True,
        )
        @given(candidate=strat)
        def _shrink(candidate: dict) -> None:
            nonlocal best
            score = self._evaluate(candidate, random.Random(self.seed))
            if (is_max and score >= baseline) or (not is_max and score <= baseline):
                if (is_max and score >= best.score) or (not is_max and score <= best.score):
                    best = ScoredScenario(scenario=candidate, score=score, seed=self.seed)
                # Trigger Hypothesis shrinking by failing the "test"
                assert False  # noqa: B011

        try:
            _shrink()
        except AssertionError:
            pass

        return best

    # ── internals ────────────────────────────────────────────────

    def _evaluate(self, scenario: dict, rng: random.Random) -> float:
        """Serialize scenario to YAML, run via batch_run, return metric score."""
        from kubesim._native import batch_run

        # Ensure at least one variant exists for batch_run
        study = scenario.get("study", scenario)
        if not study.get("variants"):
            study["variants"] = [{"name": "baseline", "scheduler": {"scoring": "LeastAllocated", "weight": 1}}]

        config_yaml = yaml.dump(scenario, default_flow_style=False)
        raw = batch_run(config_yaml, self.seeds)
        results = [dict(r) if not isinstance(r, dict) else r for r in raw]
        return float(self.metric(results))
