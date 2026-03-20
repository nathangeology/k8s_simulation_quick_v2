#!/usr/bin/env python3
"""Adversarial search for ConsolidateWhen threshold sweep pathologies.

Uses Optuna TPE to find scenarios where the cost-disruption tradeoff curve
across ConsolidateWhen thresholds is most pathological (sharp knees,
non-convexity, miscalibration, non-monotonicity).

Each trial generates a workload/cluster config, runs it at 8 thresholds +
2 reference policies (10 variants), and scores the resulting curve shape
using composite_trend_score.

See docs/adversarial-consolidate-when-proposal.md for design rationale.
"""

import copy
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

import yaml

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from kubesim._native import batch_run
from kubesim.adversarial import (
    OptunaAdversarialSearch, ScoredScenario, diverse_top_k,
    _WORKLOAD_ARCHETYPES, _INSTANCE_SPECS, _max_workload_request,
    _smallest_fitting_type, INSTANCE_TYPES,
)
from kubesim.strategies import TRAFFIC_PATTERNS, WORKLOAD_TYPES
from kubesim.trend_scoring import ThresholdResult, composite_trend_score

BASE_DIR = Path(__file__).resolve().parent.parent
SCENARIO_DIR = BASE_DIR / "scenarios" / "adversarial" / "consolidate-when"
RESULTS_DIR = BASE_DIR / "results" / "adversarial" / "consolidate-when"

THRESHOLDS = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0]

SEEDS = [42]  # Single seed during search for speed
REEVAL_SEEDS = [42, 100, 200]


def _build_variants() -> list[dict]:
    """Build the 10 variants: 8 thresholds + 2 reference policies."""
    variants = []
    for t in THRESHOLDS:
        variants.append({
            "name": f"threshold-{t:.2f}",
            "karpenter_version": "v1",
            "consolidate_when": {
                "policy": "WhenCostJustifiesDisruption",
                "decision_ratio_threshold": t,
            },
        })
    variants.append({
        "name": "when_empty",
        "karpenter_version": "v1",
        "consolidate_when": {"policy": "WhenEmpty"},
    })
    variants.append({
        "name": "when_underutilized",
        "karpenter_version": "v1",
        "consolidate_when": {"policy": "WhenEmptyOrUnderutilized"},
    })
    return variants


VARIANTS = _build_variants()


def _results_to_curve(results: list[dict]) -> tuple[
    list[ThresholdResult], ThresholdResult | None, ThresholdResult | None
]:
    """Convert batch_run results into ThresholdResult curve + reference points."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)

    def _avg(rows: list[dict], key: str) -> float:
        return sum(r.get(key, 0) for r in rows) / len(rows) if rows else 0.0

    def _disruption_rate(rows: list[dict]) -> float:
        evicted = sum(r.get("pods_evicted", 0) for r in rows)
        total = sum(r.get("running_pods", 0) + r.get("pending_pods", 0) for r in rows)
        return evicted / total if total > 0 else 0.0

    curve = []
    ref_empty = None
    ref_underutilized = None

    for vname, rows in by_variant.items():
        cost = _avg(rows, "total_cost_per_hour")
        disruption = _disruption_rate(rows)
        avail = sum(r.get("running_pods", 0) for r in rows) / max(
            sum(r.get("running_pods", 0) + r.get("pending_pods", 0) for r in rows), 1
        )
        nodes = _avg(rows, "node_count")

        if vname == "when_empty":
            ref_empty = ThresholdResult(None, "WhenEmpty", cost, disruption, avail, nodes)
        elif vname == "when_underutilized":
            ref_underutilized = ThresholdResult(
                None, "WhenEmptyOrUnderutilized", cost, disruption, avail, nodes
            )
        elif vname.startswith("threshold-"):
            t = float(vname.split("-")[1])
            curve.append(ThresholdResult(t, "WhenCostJustifiesDisruption", cost, disruption, avail, nodes))

    return curve, ref_empty, ref_underutilized


def _score_results(results: list[dict]) -> float:
    """Score batch_run results using composite_trend_score."""
    curve, ref_empty, ref_underutilized = _results_to_curve(results)
    if not curve or ref_empty is None or ref_underutilized is None:
        return 0.0
    return composite_trend_score(curve, ref_empty, ref_underutilized)


class ConsolidateWhenSearch:
    """Optuna TPE search for adversarial ConsolidateWhen scenarios."""

    def __init__(self, budget: int = 50, top_k: int = 10, max_nodes: int = 80):
        self.budget = budget
        self.top_k = top_k
        self.max_nodes = max_nodes

    def _build_scenario(self, trial) -> dict:
        """Map Optuna trial parameters to a scenario config."""
        n_pools = trial.suggest_int("n_pools", 1, 3)
        pools = []
        for p in range(n_pools):
            use_single = trial.suggest_categorical(f"pool{p}_single_fit", [True, False])
            its = None if use_single else list(INSTANCE_TYPES)
            max_n = max(10, self.max_nodes)
            pool = {
                "instance_types": its,
                "min_nodes": 0,
                "max_nodes": trial.suggest_int(f"pool{p}_max", 10, max_n),
                "karpenter": {
                    "consolidation": {
                        "policy": "WhenUnderutilized",
                        "consolidateAfter": f"{trial.suggest_int(f'pool{p}_consol_after', 0, 600)}s",
                    },
                    "disruption": {
                        "budgets": [{
                            "nodes": trial.suggest_int(f"pool{p}_disruption_nodes", 1, 10),
                        }],
                    },
                },
            }
            pools.append(pool)

        types = WORKLOAD_TYPES
        n_workloads = trial.suggest_int("n_workloads", 2, 5)
        workloads = []
        for w in range(n_workloads):
            wtype = trial.suggest_categorical(f"w{w}_type", types)
            builder = _WORKLOAD_ARCHETYPES.get(wtype)
            if builder:
                workloads.append(builder(trial, w))
            else:
                workloads.append({"type": wtype, "count": 1})

        # Add PDB coverage
        pdb_pct = trial.suggest_categorical("pdb_coverage", [0, 25, 50, 80])
        if pdb_pct > 0:
            for wi, w in enumerate(workloads):
                if trial.suggest_categorical(f"w{wi}_pdb", [True, False]):
                    w["pdb"] = {"min_available": f"{pdb_pct}%"}

        # Resolve single-fit pools
        max_cpu, max_mem = _max_workload_request(workloads)
        for pool in pools:
            if pool["instance_types"] is None:
                pool["instance_types"] = [_smallest_fitting_type(max_cpu, max_mem, INSTANCE_TYPES)]

        # Add priority mix
        priorities = ["low", "medium", "high"]
        for wi, w in enumerate(workloads):
            if "priority" not in w:
                w["priority"] = trial.suggest_categorical(f"w{wi}_prio", priorities)

        scenario = {
            "study": {
                "name": f"cw-optuna-{trial.number}",
                "runs": 20,
                "time_mode": "wall_clock",
                "scheduling_strategy": "reverse_schedule",
                "cluster": {"node_pools": pools},
                "workloads": workloads,
                "variants": VARIANTS,
                "metrics": {"compare": [
                    "total_cost_per_hour", "node_count", "running_pods",
                    "pending_pods", "pods_evicted",
                ]},
            }
        }

        # Traffic pattern
        traffic_type = trial.suggest_categorical("traffic_type", TRAFFIC_PATTERNS)
        scenario["study"]["traffic_pattern"] = {
            "type": traffic_type,
            "peak_multiplier": trial.suggest_float("traffic_peak", 1.5, 8.0),
            "duration": "24h",
        }

        # Scale-down timing
        scale_down_hour = trial.suggest_categorical("scale_down_hour", [6, 12, 18])
        for w in scenario["study"]["workloads"]:
            if w.get("type") in ("web_app", "saas_microservice"):
                w.setdefault("scale_down", [{"at": f"{scale_down_hour}h", "reduce_by": 3}])

        return scenario

    def _evaluate(self, scenario: dict) -> float:
        """Run scenario and score the threshold curve."""
        config_yaml = yaml.dump(scenario, default_flow_style=False)
        try:
            raw = batch_run(config_yaml, SEEDS)
            results = [dict(r) if not isinstance(r, dict) else r for r in raw]
        except Exception:
            return 0.0
        return _score_results(results)

    def run(self) -> list[ScoredScenario]:
        """Execute Optuna TPE search."""
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

        scored = []
        for trial in study.trials:
            if trial.value is not None and trial.value > float("-inf"):
                scenario = scenarios.get(trial.number, {})
                scored.append(ScoredScenario(scenario=scenario, score=trial.value, seed=0))

        scored.sort(key=lambda s: s.score, reverse=True)
        return diverse_top_k(scored, self.top_k)


def strip_variants(scenario: dict) -> dict:
    """Remove variants for clean scenario YAML (they're added at runtime)."""
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def main():
    import argparse

    parser = argparse.ArgumentParser(
        description="Adversarial search for ConsolidateWhen threshold pathologies (Optuna TPE)"
    )
    parser.add_argument("--budget", type=int, default=50)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--max-nodes", type=int, default=80)
    args = parser.parse_args()

    print(f"Running ConsolidateWhen adversarial search (budget={args.budget})...")
    search = ConsolidateWhenSearch(
        budget=args.budget, top_k=args.top_k, max_nodes=args.max_nodes
    )
    ranked = search.run()
    print(f"  Found {len(ranked)} scenarios")

    # Re-evaluate top scenarios with full seeds
    print(f"\nRe-evaluating top {len(ranked)} with {len(REEVAL_SEEDS)} seeds...")
    detailed: list[tuple[float, dict, list[ThresholdResult], ThresholdResult, ThresholdResult]] = []
    for scored in ranked:
        scenario = copy.deepcopy(scored.scenario)
        study = scenario.get("study", scenario)
        study["variants"] = VARIANTS
        study["scheduling_strategy"] = "reverse_schedule"
        config_yaml = yaml.dump(scenario, default_flow_style=False)
        try:
            raw = batch_run(config_yaml, REEVAL_SEEDS)
            results = [dict(r) if not isinstance(r, dict) else r for r in raw]
        except Exception:
            continue
        curve, ref_empty, ref_underutilized = _results_to_curve(results)
        if not curve or ref_empty is None or ref_underutilized is None:
            continue
        score = composite_trend_score(curve, ref_empty, ref_underutilized)
        detailed.append((score, scored.scenario, curve, ref_empty, ref_underutilized))

    detailed.sort(key=lambda x: x[0], reverse=True)
    top = detailed[:args.top_k]

    # Write scenario YAMLs
    SCENARIO_DIR.mkdir(parents=True, exist_ok=True)
    for f in SCENARIO_DIR.iterdir():
        if f.name.startswith("worst_case_") and f.suffix == ".yaml":
            f.unlink()

    print(f"\nTop {len(top)} adversarial ConsolidateWhen scenarios:")
    print(f"{'#':>3} {'Score':>8} {'Knee':>8} {'NonMono':>8} {'CostRange':>10}  File")
    print("-" * 60)

    manifest_entries = []
    for i, (score, scenario, curve, ref_empty, ref_underutilized) in enumerate(top):
        fname = f"worst_case_{i + 1:02d}.yaml"
        path = SCENARIO_DIR / fname
        clean = strip_variants(scenario)
        study_cfg = clean.get("study", clean)
        study_cfg["variants"] = VARIANTS
        study_cfg["scheduling_strategy"] = "full_scan"
        with open(path, "w") as f:
            f.write(f"# Adversarial ConsolidateWhen scenario #{i + 1} — score: {score:.4f}\n")
            yaml.dump(clean, f, default_flow_style=False, sort_keys=False)

        # Compute component scores for manifest
        cost_range = max(c.cost for c in curve) - min(c.cost for c in curve)
        threshold_pts = sorted([c for c in curve if c.threshold is not None], key=lambda c: c.threshold)
        non_mono = sum(
            1 for j in range(len(threshold_pts) - 1)
            if threshold_pts[j + 1].cost > threshold_pts[j].cost
            and threshold_pts[j + 1].disruption > threshold_pts[j].disruption
        )

        entry = {
            "filename": fname,
            "composite_score": round(score, 6),
            "cost_range": round(cost_range, 6),
            "non_monotonic_intervals": non_mono,
            "ref_empty_cost": round(ref_empty.cost, 6),
            "ref_underutilized_cost": round(ref_underutilized.cost, 6),
            "curve": [
                {
                    "threshold": c.threshold,
                    "cost": round(c.cost, 6),
                    "disruption": round(c.disruption, 6),
                    "availability": round(c.availability, 6),
                    "node_count": round(c.node_count, 2),
                }
                for c in sorted(curve, key=lambda c: c.threshold or 0)
            ],
        }
        manifest_entries.append(entry)
        print(f"{i + 1:3d} {score:>8.4f} {cost_range:>8.4f} {non_mono:>8d} {cost_range:>10.4f}  {fname}")

    # Write manifest
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "search_method": "optuna_tpe",
        "scoring": "composite_trend_score",
        "budget": args.budget,
        "thresholds": THRESHOLDS,
        "seeds_search": SEEDS,
        "seeds_reeval": REEVAL_SEEDS,
        "top_k": args.top_k,
        "scenarios": manifest_entries,
    }
    manifest_path = RESULTS_DIR / "manifest.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"\nManifest: {manifest_path}")
    print(f"Scenarios: {SCENARIO_DIR}/")


if __name__ == "__main__":
    main()
