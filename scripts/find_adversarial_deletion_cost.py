#!/usr/bin/env python3
"""Adversarial discovery: 5-way deletion cost ranking strategies.

Finds scenarios where the 5 deletion cost strategies diverge most across
availability, cost_efficiency, and disruption_rate objectives.
Focuses on scale-down events and batch job completions where deletion
ordering matters.
"""

import copy
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

import yaml

# Import _native from installed package first, then add local python/ for other modules
from kubesim._native import batch_run  # noqa: E402

# Add local python/ to path so we pick up local versions of pure-Python modules
# We must reload kubesim to pick up local modules after _native is already cached
import importlib
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))
import kubesim
importlib.reload(kubesim)

from kubesim.objectives import cost_efficiency, availability, disruption_rate  # noqa: E402
from kubesim.report import run_report  # noqa: E402
from kubesim.strategies import cluster_scenario, chaos_scenario, ALL_WORKLOAD_TYPES  # noqa: E402

BASE_DIR = Path(__file__).resolve().parent.parent
SCENARIO_DIR = BASE_DIR / "scenarios" / "adversarial" / "deletion-cost"
RESULTS_DIR = BASE_DIR / "results" / "adversarial" / "deletion-cost"

VARIANTS = [
    {"name": "baseline", "deletion_cost_strategy": "none"},
    {"name": "smallest_first", "deletion_cost_strategy": "prefer_emptying_nodes"},
    {"name": "largest_first", "deletion_cost_strategy": "largest_first"},
    {"name": "unallocated_vcpu", "deletion_cost_strategy": "unallocated_vcpu"},
    {"name": "random", "deletion_cost_strategy": "random"},
]

SEEDS = [42, 100, 200]
OBJECTIVE_FNS = {
    "availability": availability,
    "cost_efficiency": cost_efficiency,
    "disruption_rate": disruption_rate,
}


def _max_pairwise_divergence(results: list[dict]) -> float:
    """Compute max pairwise divergence across all 5 variants and all objectives."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)
    if len(by_variant) < 2:
        return 0.0

    names = list(by_variant.keys())
    max_div = 0.0
    for obj_fn in OBJECTIVE_FNS.values():
        scores = {n: obj_fn(by_variant[n]) for n in names}
        for i, a in enumerate(names):
            for b in names[i + 1:]:
                d = abs(scores[a] - scores[b])
                if d < float("inf"):
                    max_div = max(max_div, d)
    return max_div


def _per_variant_objectives(results: list[dict]) -> dict:
    """Per-variant, per-objective scores."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)
    out = {}
    for vname, vresults in by_variant.items():
        out[vname] = {}
        for oname, fn in OBJECTIVE_FNS.items():
            score = fn(vresults)
            out[vname][oname] = score if abs(score) < float("inf") else None
    return out


def _avg(lst, key):
    return sum(r.get(key, 0) for r in lst) / len(lst) if lst else 0


def evaluate(scenario: dict) -> dict | None:
    """Run scenario with all 5 variants, return divergence metrics."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    study["scheduling_strategy"] = "reverse_schedule"
    config_yaml = yaml.dump(scenario, default_flow_style=False)
    try:
        raw = batch_run(config_yaml, SEEDS)
    except Exception:
        return None

    results = [dict(r) if not isinstance(r, dict) else r for r in raw]
    by_variant = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)
    if len(by_variant) < 2:
        return None

    divergence = _max_pairwise_divergence(results)
    objectives = _per_variant_objectives(results)

    per_variant_cost = {v: _avg(rows, "total_cost_per_hour") for v, rows in by_variant.items()}

    return {
        "divergence": divergence,
        "objectives": objectives,
        "per_variant_cost": per_variant_cost,
    }


def strip_variants(scenario: dict) -> dict:
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def run_search(strat, budget, label):
    from hypothesis import HealthCheck, given, settings

    scored = []
    counter = {"n": 0}

    @settings(
        max_examples=budget,
        database=None,
        suppress_health_check=[HealthCheck.too_slow],
        derandomize=True,
        deadline=None,
    )
    @given(scenario=strat)
    def search(scenario):
        if counter["n"] >= budget:
            return
        counter["n"] += 1
        metrics = evaluate(scenario)
        if metrics:
            scored.append((metrics, scenario))
        if counter["n"] % 100 == 0:
            best = max((s[0]["divergence"] for s in scored), default=0)
            print(f"  [{label}] {counter['n']}/{budget}, best divergence: {best:.6f}")

    print(f"Running {label} search (budget={budget})...")
    try:
        search()
    except Exception as e:
        if scored:
            print(f"  [{label}] stopped at {counter['n']}: {type(e).__name__}")
        else:
            raise

    return scored


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Adversarial finder: 5-way deletion cost strategies")
    parser.add_argument("--budget", type=int, default=500)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--report-seeds", type=int, default=50)
    parser.add_argument("--report-top", type=int, default=5)
    args = parser.parse_args()

    # Focus on scenarios with scale-down events and batch jobs
    normal_strat = cluster_scenario(
        max_nodes=200,
        workload_types=ALL_WORKLOAD_TYPES,
        min_workloads=2,
        max_workloads=8,
        min_pools=1,
        max_pools=3,
    )
    chaos_strat = chaos_scenario(max_nodes=200)

    normal_scored = run_search(normal_strat, args.budget, "normal+expanded")
    chaos_scored = run_search(chaos_strat, args.budget, "chaos")

    all_scored = normal_scored + chaos_scored
    print(f"\nTotal evaluated: {len(all_scored)} scenarios")

    # Rank by max pairwise divergence
    all_scored.sort(key=lambda x: x[0]["divergence"], reverse=True)
    top = all_scored[:args.top_k]

    # Save scenarios
    SCENARIO_DIR.mkdir(parents=True, exist_ok=True)
    for f in SCENARIO_DIR.iterdir():
        if f.name.startswith("worst_case_") and f.suffix == ".yaml":
            f.unlink()

    print(f"\nTop {len(top)} scenarios by max pairwise divergence:")
    print(f"{'#':>3} {'Divergence':>12}  {'Best Strategy':>20}  {'Worst Strategy':>20}  File")
    print("-" * 85)

    manifest_entries = []
    for i, (metrics, scenario) in enumerate(top):
        fname = f"worst_case_{i + 1:02d}.yaml"
        path = SCENARIO_DIR / fname
        clean = strip_variants(scenario)
        # Re-inject all 5 variants for reproducibility
        study = clean.get("study", clean)
        study["variants"] = VARIANTS
        with open(path, "w") as f:
            f.write(f"# Adversarial scenario #{i + 1} — divergence: {metrics['divergence']:.4f}\n")
            f.write("# 5-way deletion cost ranking comparison\n")
            yaml.dump(clean, f, default_flow_style=False, sort_keys=False)

        # Find best/worst strategy by cost
        costs = metrics["per_variant_cost"]
        best_strat = min(costs, key=costs.get) if costs else "?"
        worst_strat = max(costs, key=costs.get) if costs else "?"

        manifest_entries.append({
            "filename": fname,
            "divergence": round(metrics["divergence"], 6),
            "per_variant_cost": {k: round(v, 6) for k, v in costs.items()},
            "objectives": {
                vname: {oname: round(s, 6) if s is not None else None for oname, s in oscores.items()}
                for vname, oscores in metrics["objectives"].items()
            },
        })
        print(f"{i + 1:3d} {metrics['divergence']:>12.4f}  {best_strat:>20}  {worst_strat:>20}  {fname}")

    # Write manifest
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "budget": args.budget,
        "seeds": SEEDS,
        "top_k": args.top_k,
        "objectives": list(OBJECTIVE_FNS.keys()),
        "variants": [v["name"] for v in VARIANTS],
        "scenarios": manifest_entries,
    }
    manifest_path = RESULTS_DIR / "manifest.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"\nManifest: {manifest_path}")

    # Run reports on top N
    report_n = min(args.report_top, len(top))
    print(f"\nRunning reports on top {report_n} scenarios with {args.report_seeds} seeds...")
    for i in range(report_n):
        scenario_path = SCENARIO_DIR / f"worst_case_{i + 1:02d}.yaml"
        out_dir = str(RESULTS_DIR)
        print(f"  Report {i + 1}/{report_n}: {scenario_path.name}")
        try:
            run_report(str(scenario_path), seeds=args.report_seeds, output_dir=out_dir)
        except Exception as e:
            print(f"    WARNING: report failed: {e}")

    # Summary stats
    all_divs = [s[0]["divergence"] for s in all_scored]
    nonzero = [d for d in all_divs if d > 0]
    print(f"\nSummary:")
    print(f"  Total scenarios: {len(all_divs)}")
    print(f"  With divergence > 0: {len(nonzero)}")
    if nonzero:
        print(f"  Max divergence: {max(nonzero):.6f}")
        print(f"  Mean (nonzero): {sum(nonzero) / len(nonzero):.6f}")

    print(f"\nScenarios: {SCENARIO_DIR}/")
    print(f"Results: {RESULTS_DIR}/")


if __name__ == "__main__":
    main()
