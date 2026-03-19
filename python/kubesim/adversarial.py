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

import math
import random
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Callable, Literal

import yaml

from kubesim.strategies import (
    cluster_scenario, chaos_scenario,
    INSTANCE_TYPES, TINY_INSTANCE_TYPES, CONSOLIDATION_POLICIES,
    TRAFFIC_PATTERNS, SCALE_DOWN_PATTERNS, ALL_WORKLOAD_TYPES, WORKLOAD_TYPES,
)


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
    name_b="prefer_emptying_nodes",
    config_b={"name": "prefer_emptying_nodes", "deletion_cost_strategy": "prefer_emptying_nodes"},
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
    # Karpenter config dimensions
    for p in pools:
        karp = p.get("karpenter", {})
        if karp:
            consol = karp.get("consolidation", {})
            ca = consol.get("consolidateAfter", "0s")
            features["max_consol_after_s"] = max(features.get("max_consol_after_s", 0), int(ca.rstrip("s") or 0))
            budgets = karp.get("disruption", {}).get("budgets", [{}])
            if budgets:
                features["max_disruption_nodes"] = max(features.get("max_disruption_nodes", 0), budgets[0].get("nodes", 0))
                features["max_disruption_pct"] = max(features.get("max_disruption_pct", 0), budgets[0].get("percent", 0))
            ea = karp.get("expireAfter", "0h")
            features["max_expire_h"] = max(features.get("max_expire_h", 0), int(ea.rstrip("h") or 0))
            bi = karp.get("batchIdleDuration", "0s")
            features["max_batch_idle_s"] = max(features.get("max_batch_idle_s", 0), int(bi.rstrip("s") or 0))
    return features


def diverse_top_k(scored: list[ScoredScenario], k: int) -> list[ScoredScenario]:
    """Select top-k scenarios maximizing score × diversity in feature space.

    Greedy selection: start with the highest-scoring scenario, then iteratively
    pick the candidate that maximizes ``score × min_distance_to_selected``
    (Euclidean distance in normalized feature space).
    """
    if len(scored) <= k:
        return list(scored)

    # Extract and normalize feature vectors
    raw_features = [_extract_features(s.scenario) for s in scored]
    all_keys = sorted({fk for f in raw_features for fk in f})
    if not all_keys:
        return scored[:k]

    vectors = [[f.get(key, 0.0) for key in all_keys] for f in raw_features]

    # Compute per-feature min/max for normalization
    mins = [min(v[i] for v in vectors) for i in range(len(all_keys))]
    maxs = [max(v[i] for v in vectors) for i in range(len(all_keys))]
    ranges = [mx - mn if mx > mn else 1.0 for mn, mx in zip(mins, maxs)]

    norm = [[((v[i] - mins[i]) / ranges[i]) for i in range(len(all_keys))] for v in vectors]

    def _dist(a: list[float], b: list[float]) -> float:
        return math.sqrt(sum((x - y) ** 2 for x, y in zip(a, b)))

    # Greedy diverse selection
    selected_idx: list[int] = [0]  # Start with highest-scoring (already sorted)
    remaining = set(range(1, len(scored)))

    for _ in range(k - 1):
        if not remaining:
            break
        best_idx = -1
        best_val = -1.0
        for idx in remaining:
            min_d = min(_dist(norm[idx], norm[s]) for s in selected_idx)
            val = scored[idx].score * min_d
            if val > best_val:
                best_val = val
                best_idx = idx
        selected_idx.append(best_idx)
        remaining.discard(best_idx)

    return [scored[i] for i in selected_idx]


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
    progress: bool = True

    # ── public API ───────────────────────────────────────────────

    def run(self) -> list[ScoredScenario]:
        """Execute the search and return ranked extreme scenarios."""
        import sys
        import time as _time
        from hypothesis import given, settings, HealthCheck

        rng = random.Random(self.seed)
        strat = self._build_strategy()

        found: list[ScoredScenario] = []
        counter = {"n": 0}
        best_score = {"val": float("-inf") if self.objective == "maximize" else float("inf")}
        t_start = _time.perf_counter()
        t_last = {"val": t_start}
        recent_times: list[float] = []
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

            # Update best score
            is_max = self.objective == "maximize"
            if (is_max and score > best_score["val"]) or (not is_max and score < best_score["val"]):
                best_score["val"] = score

            # Progress output
            if self.progress:
                now = _time.perf_counter()
                dt = now - t_last["val"]
                t_last["val"] = now
                recent_times.append(dt)
                if len(recent_times) > 10:
                    recent_times.pop(0)

                elapsed = now - t_start
                n = counter["n"]
                avg_recent = sum(recent_times) / len(recent_times)
                remaining = self.budget - n
                eta = remaining * avg_recent

                bar_width = 30
                filled = int(bar_width * n / self.budget)
                bar = "█" * filled + "░" * (bar_width - filled)
                pct = n * 100 // self.budget
                best_str = f"{best_score['val']:.4f}" if abs(best_score["val"]) < float("inf") else "—"
                print(f"\r  {bar} {pct:>3}% ({n}/{self.budget})  "
                      f"best={best_str}  "
                      f"elapsed={elapsed:.0f}s  "
                      f"ETA ~{eta:.0f}s  "
                      f"last={dt:.2f}s  ",
                      end="", flush=True, file=sys.stderr)

        self._last_results: list[dict] = []
        try:
            _search()
        except AssertionError:
            pass

        if self.progress:
            elapsed = _time.perf_counter() - t_start
            print(f"\r  Done: {counter['n']} scenarios in {elapsed:.1f}s "
                  f"({counter['n']/elapsed:.1f}/s)  "
                  f"best={best_score['val']:.4f}          ",
                  file=sys.stderr)

        reverse = self.objective == "maximize"
        found.sort(key=lambda s: s.score, reverse=reverse)
        result = diverse_top_k(found, self.top_k)

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


# ── Optuna-based Bayesian search ─────────────────────────────────

# Workload archetype builders for Optuna (mirror Hypothesis strategies)
_WORKLOAD_ARCHETYPES = {
    "web_app": lambda trial, i: {
        "type": "web_app",
        "count": trial.suggest_int(f"w{i}_count", 1, 20),
        "replicas": {
            "min": trial.suggest_int(f"w{i}_rep_min", 2, 10),
            "max": trial.suggest_int(f"w{i}_rep_max", 11, 50),
        },
        "churn": "low",
        "traffic": "diurnal",
    },
    "batch_job": lambda trial, i: {
        "type": "batch_job",
        "count": trial.suggest_int(f"w{i}_count", 1, 30),
        "priority": "low",
    },
    "ml_training": lambda trial, i: {
        "type": "ml_training",
        "count": trial.suggest_int(f"w{i}_count", 1, 10),
        "replicas": {"fixed": 1},
        "priority": "high",
    },
    "saas_microservice": lambda trial, i: {
        "type": "saas_microservice",
        "count": trial.suggest_int(f"w{i}_count", 1, 15),
        "replicas": {
            "min": trial.suggest_int(f"w{i}_rep_min", 3, 10),
            "max": trial.suggest_int(f"w{i}_rep_max", 11, 200),
        },
        "churn": trial.suggest_categorical(f"w{i}_churn", ["low", "medium"]),
    },
    "overcommit": lambda trial, i: {
        "type": "batch_job",
        "count": trial.suggest_int(f"w{i}_count", 20, 100),
        "priority": trial.suggest_categorical(f"w{i}_prio", ["low", "medium", "high"]),
        "cpu_request": {"dist": "uniform", "min": "8000m", "max": "8000m"},
        "memory_request": {"dist": "uniform", "min": "32768Mi", "max": "32768Mi"},
    },
    "anti_affinity": lambda trial, i: {
        "type": "web_app",
        "count": trial.suggest_int(f"w{i}_count", 1, 5),
        "replicas": {
            "min": trial.suggest_int(f"w{i}_rep_min", 3, 20),
            "max": trial.suggest_int(f"w{i}_rep_max", 20, 50),
        },
        "churn": "low",
        "traffic": "steady",
        "pod_anti_affinity": {"topology_key": "kubernetes.io/hostname"},
    },
    "varying_batch": lambda trial, i: {
        "type": "batch_job",
        "count": trial.suggest_int(f"w{i}_count", 5, 50),
        "priority": trial.suggest_categorical(f"w{i}_prio", ["low", "medium", "high"]),
    },
    "gpu_on_non_gpu": lambda trial, i: {
        "type": "ml_training",
        "count": trial.suggest_int(f"w{i}_count", 1, 5),
        "replicas": {"fixed": 1},
        "priority": "high",
        "gpu_request": {"dist": "choice", "values": [1, 2, 4, 8]},
    },
    "extreme_replicas": lambda trial, i: {
        "type": "web_app",
        "count": 1,
        "replicas": {
            "min": trial.suggest_int(f"w{i}_rep_min", 50, 200),
            "max": trial.suggest_int(f"w{i}_rep_max", 200, 400),
        },
        "churn": "low",
        "traffic": "steady",
    },
}


# Instance type specs: (vCPU_millicores, memory_MiB)
_INSTANCE_SPECS: dict[str, tuple[int, int]] = {
    "t3.micro": (2000, 1024), "t3.small": (2000, 2048), "t3.medium": (2000, 4096),
    "m5.large": (2000, 8192), "m5.xlarge": (4000, 16384), "m5.2xlarge": (8000, 32768),
    "m5.4xlarge": (16000, 65536),
    "c5.large": (2000, 4096), "c5.xlarge": (4000, 8192), "c5.2xlarge": (8000, 16384),
    "c5.4xlarge": (16000, 32768),
    "r5.large": (2000, 16384), "r5.xlarge": (4000, 32768), "r5.2xlarge": (8000, 65536),
    "m6i.large": (2000, 8192), "m6i.xlarge": (4000, 16384), "m6i.2xlarge": (8000, 32768),
    "c6i.large": (2000, 4096), "c6i.xlarge": (4000, 8192), "c6i.2xlarge": (8000, 16384),
    "p3.2xlarge": (8000, 62464), "p3.8xlarge": (32000, 249856),
    "g4dn.xlarge": (4000, 16384), "g4dn.2xlarge": (8000, 32768),
}

# Archetype default resource requests: (cpu_millicores, memory_MiB)
_ARCHETYPE_RESOURCE_DEFAULTS: dict[str, tuple[int, int]] = {
    "web_app": (250, 256), "batch_job": (1000, 2048),
    "ml_training": (8000, 32768), "saas_microservice": (500, 512),
}


def _parse_cpu_millicores(s: str) -> int:
    """Parse a CPU request string to millicores."""
    if s.endswith("m"):
        return int(s[:-1])
    return int(float(s) * 1000)


def _parse_memory_mib(s: str) -> int:
    """Parse a memory request string to MiB."""
    if s.endswith("Mi"):
        return int(s[:-2])
    if s.endswith("Gi"):
        return int(float(s[:-2]) * 1024)
    return int(s) // (1024 * 1024)


def _max_workload_request(workloads: list[dict]) -> tuple[int, int]:
    """Return (max_cpu_millicores, max_memory_mib) across workloads."""
    max_cpu, max_mem = 0, 0
    for w in workloads:
        wtype = w.get("type", "")
        cpu_dist = w.get("cpu_request")
        mem_dist = w.get("memory_request")
        if cpu_dist and isinstance(cpu_dist, dict):
            cpu = _parse_cpu_millicores(cpu_dist.get("max", cpu_dist.get("min", "500m")))
        else:
            cpu = _ARCHETYPE_RESOURCE_DEFAULTS.get(wtype, (500, 512))[0]
        if mem_dist and isinstance(mem_dist, dict):
            mem = _parse_memory_mib(mem_dist.get("max", mem_dist.get("min", "512Mi")))
        else:
            mem = _ARCHETYPE_RESOURCE_DEFAULTS.get(wtype, (500, 512))[1]
        max_cpu = max(max_cpu, cpu)
        max_mem = max(max_mem, mem)
    return max_cpu, max_mem


def _smallest_fitting_type(cpu_m: int, mem_m: int, candidates: list[str]) -> str:
    """Return the smallest instance type from candidates that fits the request.

    "Smallest" = least total resource (cpu + normalized memory). Falls back to
    the first candidate if nothing fits.
    """
    best, best_size = None, float("inf")
    for it in candidates:
        spec = _INSTANCE_SPECS.get(it)
        if not spec:
            continue
        if spec[0] >= cpu_m and spec[1] >= mem_m:
            size = spec[0] + spec[1]  # simple combined metric
            if size < best_size:
                best, best_size = it, size
    return best or candidates[0]


@dataclass
class OptunaAdversarialSearch:
    """Bayesian optimization search for adversarial scenarios using Optuna TPE.

    Drop-in replacement for the Hypothesis-based search loop. Reuses existing
    objective functions, batch_run, and scenario infrastructure.

    Args:
        objective_fn: Callable receiving list of result dicts, returning float to maximize.
        seeds: Seeds passed to ``batch_run`` per evaluation.
        budget: Maximum number of trials (scenario evaluations).
        screen_threshold: Quick-screen cutoff for progressive eval.
        top_k: Number of top scenarios to return.
        workload_types: Workload archetypes to include in search space.
        max_pools: Maximum node pools per scenario.
        max_nodes: Maximum nodes per pool.
        variant_pair: Optional variant pair to inject into scenarios.
        chaos: If True, use chaos-style search space.
        random_node_mix: If True, use old behavior (random instance type subsets).
            If False (default), each pool uses all types or smallest-fitting single type.
    """

    objective_fn: Callable[[list[dict]], float]
    seeds: list[int] = field(default_factory=lambda: [42])
    budget: int = 500
    screen_threshold: float = 0.01
    top_k: int = 10
    workload_types: list[str] | None = None
    max_pools: int = 3
    max_nodes: int = 200
    variant_pair: VariantPair | None = None
    chaos: bool = False
    random_node_mix: bool = False

    def _build_scenario(self, trial) -> dict:
        """Map Optuna trial parameters to a scenario config dict."""
        import optuna

        n_pools = trial.suggest_int("n_pools", 1, self.max_pools)
        pools = []
        for p in range(n_pools):
            if self.chaos:
                # Single-instance pool for chaos
                it = trial.suggest_categorical(f"pool{p}_it", INSTANCE_TYPES + TINY_INSTANCE_TYPES)
                pool = {
                    "instance_types": [it],
                    "min_nodes": 1,
                    "max_nodes": trial.suggest_int(f"pool{p}_max", 5, self.max_nodes),
                }
            elif self.random_node_mix:
                # Old behavior: 80% all types, 20% random restricted subset
                restrict = trial.suggest_categorical(f"pool{p}_restrict", [False, False, False, False, True])
                if restrict:
                    n_types = trial.suggest_int(f"pool{p}_n_types", 1, 6)
                    its = []
                    for t in range(n_types):
                        idx = trial.suggest_int(f"pool{p}_it{t}", 0, len(INSTANCE_TYPES) - 1)
                        it = INSTANCE_TYPES[idx]
                        if it not in its:
                            its.append(it)
                    if not its:
                        its = [INSTANCE_TYPES[0]]
                else:
                    its = list(INSTANCE_TYPES)
                max_n = max(5, self.max_nodes)
                min_cap = min(10, max_n - 1)
                pool = {
                    "instance_types": its,
                    "min_nodes": trial.suggest_int(f"pool{p}_min", 0, max(0, min_cap)),
                    "max_nodes": trial.suggest_int(f"pool{p}_max", max(1, min_cap + 1), max_n),
                }
            else:
                # Default: all types or smallest-fitting single type
                # (workloads built below — deferred resolution for single-fit pools)
                use_single = trial.suggest_categorical(f"pool{p}_single_fit", [True, False])
                its = None if use_single else list(INSTANCE_TYPES)
                max_n = max(5, self.max_nodes)
                min_cap = min(10, max_n - 1)
                pool = {
                    "instance_types": its,
                    "min_nodes": trial.suggest_int(f"pool{p}_min", 0, max(0, min_cap)),
                    "max_nodes": trial.suggest_int(f"pool{p}_max", max(1, min_cap + 1), max_n),
                }
            # Optional karpenter config
            if trial.suggest_categorical(f"pool{p}_karp", [True, False]):
                pool["karpenter"] = {
                    "consolidation": {
                        "policy": trial.suggest_categorical(
                            f"pool{p}_consol", CONSOLIDATION_POLICIES
                        ),
                        "consolidateAfter": f"{trial.suggest_int(f'pool{p}_consol_after_s', 0, 1800)}s",
                    },
                    "disruption": {
                        "budgets": [{
                            "nodes": trial.suggest_int(f"pool{p}_disruption_nodes", 1, 10),
                            "percent": trial.suggest_int(f"pool{p}_disruption_pct", 5, 50),
                        }],
                    },
                    "expireAfter": f"{trial.suggest_int(f'pool{p}_expire_h', 1, 720)}h",
                    "batchIdleDuration": f"{trial.suggest_int(f'pool{p}_batch_idle_s', 1, 30)}s",
                }
            pools.append(pool)

        types = self.workload_types or (ALL_WORKLOAD_TYPES if self.chaos else WORKLOAD_TYPES)
        n_workloads = trial.suggest_int("n_workloads", 2, 6)
        workloads = []
        for w in range(n_workloads):
            wtype = trial.suggest_categorical(f"w{w}_type", types)
            builder = _WORKLOAD_ARCHETYPES.get(wtype)
            if builder:
                workloads.append(builder(trial, w))
            else:
                workloads.append({"type": wtype, "count": 1})

        time_mode = "wall_clock"  # logical mode causes consolidation thrash (33M+ events)

        # Resolve deferred single-fit pools now that workloads are known
        if not self.random_node_mix and not self.chaos:
            max_cpu, max_mem = _max_workload_request(workloads)
            for pool in pools:
                if pool["instance_types"] is None:
                    pool["instance_types"] = [_smallest_fitting_type(max_cpu, max_mem, INSTANCE_TYPES)]

        scenario = {
            "study": {
                "name": f"optuna-{trial.number}",
                "runs": 20,  # was 50 — reduce to limit memory; increase after resource monitor lands
                "time_mode": time_mode,
                "scheduling_strategy": "reverse_schedule",
                "cluster": {"node_pools": pools},
                "workloads": workloads,
                "variants": [],
                "metrics": {"compare": []},
            }
        }

        # Traffic pattern
        if self.chaos or trial.suggest_categorical("has_traffic", [True, False]):
            scenario["study"]["traffic_pattern"] = {
                "type": trial.suggest_categorical("traffic_type", TRAFFIC_PATTERNS),
                "peak_multiplier": trial.suggest_float("traffic_peak", 1.5, 10.0),
                "duration": trial.suggest_categorical("traffic_dur", ["12h", "24h", "48h"]),
            }

        if self.chaos:
            scenario["study"]["scale_down_pattern"] = trial.suggest_categorical(
                "scale_down", SCALE_DOWN_PATTERNS
            )

        return scenario

    def _evaluate(self, scenario: dict) -> float:
        """Evaluate a scenario with progressive screening."""
        from kubesim._native import batch_run

        study = scenario.get("study", scenario)

        if self.variant_pair:
            study["variants"] = [self.variant_pair.config_a, self.variant_pair.config_b]
        elif not study.get("variants") or study["variants"] == []:
            study["variants"] = [
                {"name": "baseline", "scheduler": {"scoring": "LeastAllocated", "weight": 1}}
            ]

        study["scheduling_strategy"] = "reverse_schedule"
        config_yaml = yaml.dump(scenario, default_flow_style=False)

        # Phase 1: Quick screen
        quick_raw = batch_run(config_yaml, [self.seeds[0]])
        quick_results = [dict(r) if not isinstance(r, dict) else r for r in quick_raw]
        quick_score = float(self.objective_fn(quick_results))
        if abs(quick_score) < self.screen_threshold:
            return quick_score

        # Phase 2: Full evaluation
        raw = batch_run(config_yaml, self.seeds)
        results = [dict(r) if not isinstance(r, dict) else r for r in raw]
        return float(self.objective_fn(results))

    def run(self) -> list[ScoredScenario]:
        """Execute Optuna TPE search and return top-k scored scenarios."""
        import optuna

        optuna.logging.set_verbosity(optuna.logging.WARNING)
        study = optuna.create_study(direction="maximize")

        scenarios: dict[int, dict] = {}

        def objective(trial):
            try:
                scenario = self._build_scenario(trial)
            except Exception:
                return float("-inf")
            scenarios[trial.number] = scenario
            try:
                return self._evaluate(scenario)
            except Exception:
                return float("-inf")

        study.optimize(objective, n_trials=self.budget, catch=(Exception,))

        # Collect all completed trials as ScoredScenario
        scored = []
        for trial in study.trials:
            if trial.value is not None and trial.value > float("-inf"):
                scenario = scenarios.get(trial.number, {})
                scored.append(ScoredScenario(
                    scenario=scenario,
                    score=trial.value,
                    seed=0,
                ))

        scored.sort(key=lambda s: s.score, reverse=True)
        return diverse_top_k(scored, self.top_k)
