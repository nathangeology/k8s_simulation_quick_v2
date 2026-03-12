#!/usr/bin/env python3
"""Adversarial finder: 5-way deletion cost ranking strategy comparison.

Sweeps random cluster scenarios focused on scale-down events and batch job
completions, comparing all 5 deletion cost strategies:
  baseline (none), smallest_first, largest_first, unallocated_vcpu, random

Ranks scenarios by max pairwise divergence across objectives:
  availability, cost_efficiency, disruption_rate
"""

from __future__ import annotations

import json
import os
import sys
from datetime import datetime, timezone

import yaml

from kubesim._native import batch_run
from kubesim.objectives import availability, cost_efficiency, disruption_rate
from kubesim.strategies import cluster_scenario

BASE_DIR = os.path.join(os.path.dirname(__file__), "..")
SCENARIO_DIR = os.path.join(BASE_DIR, "scenarios", "adversarial", "deletion-cost")
RESULTS_DIR = os.path.join(BASE_DIR, "results", "adversarial-deletion-cost")

VARIANTS = [
    {"name": "baseline", "deletion_cost_strategy": "none"},
    {"name": "smallest_first", "deletion_cost_strategy": "prefer_emptying_nodes"},
    {"name": "largest_first", "deletion_cost_strategy": "largest_first"},
    {"name": "unallocated_vcpu", "deletion_cost_strategy": "unallocated_vcpu"},
    {"name": "random", "deletion_cost_strategy": "random"},
]

VARIANT_NAMES = [v["name"] for v in VARIANTS]

OBJECTIVES = {
    "availability": availability,
    "cost_efficiency": cost_efficiency,
    "disruption_rate": disruption_rate,
}

SEEDS = [42, 123, 7]


def _ensure_scale_down(scenario: dict) -> dict:
    """Inject scale-down events into workloads that lack them."""
    study = scenario.get("study", scenario)
    for wl in study.get("workloads", []):
        if wl.get("type") == "batch_job":
            continue  # batch jobs complete naturally
        if "scale_down" not in wl:
            # Add a scale-down at tick 50 reducing by ~60%
            reps = wl.get("replicas", {})
            max_r = reps.get("max", reps.get("fixed", 10))
            reduce = max(1, int(max_r * 0.6))
            wl["scale_down"] = [{"at": "50", "reduce_by": reduce}]
    return scenario


def evaluate(scenario: dict, seeds: list[int]) -> dict | None:
    """Run scenario with all 5 variants, return per-variant objective scores."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    config_yaml = yaml.dump(scenario, default_flow_style=False)
    try:
        raw = batch_run(config_yaml, seeds)
    except Exception:
        return None

    rows = [dict(r) if not isinstance(r, dict) else r for r in raw]
    by_variant: dict[str, list[dict]] = {}
    for r in rows:
        by_variant.setdefault(r.get("variant", ""), []).append(r)

    if len(by_variant) < 5:
        return None

    # Compute objectives per variant
    scores: dict[str, dict[str, float]] = {}
    for vname, vrows in by_variant.items():
        scores[vname] = {oname: ofn(vrows) for oname, ofn in OBJECTIVES.items()}

    # Max pairwise divergence across all objectives
    max_div = 0.0
    for oname in OBJECTIVES:
        vals = [scores[v][oname] for v in VARIANT_NAMES]
        div = max(vals) - min(vals)
        max_div = max(max_div, div)

    # Avg metrics per variant
    def avg(lst, key):
        return sum(r.get(key, 0) for r in lst) / len(lst) if lst else 0

    variant_metrics = {}
    for vname, vrows in by_variant.items():
        variant_metrics[vname] = {
            "cost_per_hour": avg(vrows, "total_cost_per_hour"),
            "running_pods": avg(vrows, "running_pods"),
            "pending_pods": avg(vrows, "pending_pods"),
            "node_count": avg(vrows, "node_count"),
            "pods_evicted": avg(vrows, "pods_evicted"),
        }

    return {
        "max_divergence": max_div,
        "objective_scores": scores,
        "variant_metrics": variant_metrics,
    }


def main():
    from hypothesis import HealthCheck, given, settings

    import argparse
    parser = argparse.ArgumentParser(description="5-way deletion cost adversarial finder")
    parser.add_argument("--budget", type=int, default=500)
    parser.add_argument("--top-k", type=int, default=10)
    args = parser.parse_args()

    budget = args.budget
    top_k = args.top_k

    # Strategy focused on scale-down and batch job scenarios
    strat = cluster_scenario(
        max_nodes=200,
        min_workloads=2,
        max_workloads=6,
        min_pools=1,
        max_pools=3,
    )

    scored = []
    counter = {"n": 0}

    @settings(
        max_examples=budget,
        database=None,
        suppress_health_check=[HealthCheck.too_slow],
        derandomize=True,
    )
    @given(scenario=strat)
    def search(scenario):
        if counter["n"] >= budget:
            return
        counter["n"] += 1
        scenario = _ensure_scale_down(scenario)
        result = evaluate(scenario, SEEDS)
        if result:
            scored.append((result, scenario))
        if counter["n"] % 50 == 0:
            best = max((s[0]["max_divergence"] for s in scored), default=0)
            print(f"  {counter['n']}/{budget} evaluated, best divergence: {best:.6f}")

    print(f"Running 5-way deletion cost adversarial search (budget={budget})...")
    try:
        search()
    except Exception as e:
        if scored:
            print(f"  search stopped at {counter['n']}: {type(e).__name__}")
        else:
            raise

    # Rank by max divergence, keep top-k
    scored.sort(key=lambda x: x[0]["max_divergence"], reverse=True)
    top = scored[:top_k]

    # Write scenario YAMLs
    os.makedirs(SCENARIO_DIR, exist_ok=True)
    os.makedirs(RESULTS_DIR, exist_ok=True)

    # Clean old files
    for f in os.listdir(SCENARIO_DIR):
        if f.endswith(".yaml"):
            os.remove(os.path.join(SCENARIO_DIR, f))

    manifest_entries = []
    print(f"\nTop {len(top)} scenarios by max divergence:")
    print(f"{'#':>3} {'MaxDiv':>10}  {'Avail spread':>14} {'Cost spread':>12} {'Disrupt spread':>15}")
    print("-" * 60)

    for i, (result, scenario) in enumerate(top):
        fname = f"scenario_{i+1:02d}.yaml"
        path = os.path.join(SCENARIO_DIR, fname)
        # Write clean scenario (without variants injected)
        study = scenario.get("study", scenario)
        study.pop("variants", None)
        study.pop("metrics", None)
        with open(path, "w") as f:
            yaml.dump(scenario, f, default_flow_style=False, sort_keys=False)

        obj = result["objective_scores"]
        avail_spread = max(obj[v]["availability"] for v in VARIANT_NAMES) - min(obj[v]["availability"] for v in VARIANT_NAMES)
        cost_spread = max(obj[v]["cost_efficiency"] for v in VARIANT_NAMES) - min(obj[v]["cost_efficiency"] for v in VARIANT_NAMES)
        disrupt_spread = max(obj[v]["disruption_rate"] for v in VARIANT_NAMES) - min(obj[v]["disruption_rate"] for v in VARIANT_NAMES)

        print(f"{i+1:3d} {result['max_divergence']:>10.6f}  {avail_spread:>14.6f} {cost_spread:>12.6f} {disrupt_spread:>15.6f}")

        manifest_entries.append({
            "filename": fname,
            "max_divergence": round(result["max_divergence"], 6),
            "objective_scores": {v: {k: round(val, 6) for k, val in obj[v].items()} for v in VARIANT_NAMES},
            "variant_metrics": {v: {k: round(val, 4) for k, val in result["variant_metrics"][v].items()} for v in VARIANT_NAMES},
        })

    # Write manifest
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "budget": budget,
        "seeds": SEEDS,
        "variants": VARIANT_NAMES,
        "objectives": list(OBJECTIVES.keys()),
        "top_k": top_k,
        "scenarios": manifest_entries,
    }
    manifest_path = os.path.join(RESULTS_DIR, "manifest.json")
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)

    print(f"\nScenarios: {os.path.abspath(SCENARIO_DIR)}/")
    print(f"Manifest: {os.path.abspath(manifest_path)}")
    print(f"Evaluated {counter['n']} scenarios, {len(scored)} valid, saved top {len(top)}")


if __name__ == "__main__":
    main()
