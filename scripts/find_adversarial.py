#!/usr/bin/env python3
"""Find adversarial scenarios where MostAllocated vs LeastAllocated diverges most.

Sweeps random cluster scenarios via Hypothesis strategies, injects both
scheduling variants, runs them through kubesim batch_run, and ranks by
signed divergence (most_cost - least_cost) with directional categorization.

Output structure:
  scenarios/adversarial/*.yaml       — clean input YAMLs (no results)
  results/adversarial/manifest.json  — scores, direction, per-variant metrics
  results/adversarial/summary.md     — human-readable summary
"""

import json
import os
import sys
from collections import defaultdict

import yaml

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from kubesim._native import batch_run
from kubesim.strategies import cluster_scenario

BUDGET = 1000
TOP_K = 10
SEEDS = [42, 100, 200]
BASE_DIR = os.path.join(os.path.dirname(__file__), "..")
SCENARIO_DIR = os.path.join(BASE_DIR, "scenarios", "adversarial")
RESULTS_DIR = os.path.join(BASE_DIR, "results", "adversarial")

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]


def categorize(most_cost: float, least_cost: float) -> str:
    """Categorize based on signed delta: most_cost - least_cost."""
    delta = most_cost - least_cost
    if delta > 0:
        return "adversarial_to_most"
    elif delta < 0:
        return "adversarial_to_least"
    return "both_degrade"


def evaluate(scenario: dict) -> dict:
    """Run scenario with both variants, return signed divergence metrics."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    config_yaml = yaml.dump(scenario, default_flow_style=False)
    try:
        results = batch_run(config_yaml, SEEDS)
    except Exception:
        return {"score": 0.0, "signed_delta": 0.0, "category": "both_degrade"}

    rows = [dict(r) if not isinstance(r, dict) else r for r in results]
    most = [r for r in rows if r["variant"] == "most_allocated"]
    least = [r for r in rows if r["variant"] == "least_allocated"]
    if not most or not least:
        return {"score": 0.0, "signed_delta": 0.0, "category": "both_degrade"}

    def avg(lst, key):
        return sum(r[key] for r in lst) / len(lst)

    most_cost = avg(most, "total_cost_per_hour")
    least_cost = avg(least, "total_cost_per_hour")
    signed_delta = most_cost - least_cost
    category = categorize(most_cost, least_cost)

    return {
        "score": abs(signed_delta),
        "signed_delta": signed_delta,
        "category": category,
        "most_cost": most_cost,
        "least_cost": least_cost,
        "most_running": avg(most, "running_pods"),
        "least_running": avg(least, "running_pods"),
        "most_pending": avg(most, "pending_pods"),
        "least_pending": avg(least, "pending_pods"),
        "most_nodes": avg(most, "node_count"),
        "least_nodes": avg(least, "node_count"),
    }


def strip_variants(scenario: dict) -> dict:
    """Return a copy of scenario with variants removed (clean input)."""
    import copy
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def main():
    from hypothesis import given, settings, HealthCheck

    strat = cluster_scenario(
        max_nodes=200,
        min_workloads=1,
        max_workloads=8,
        min_pools=1,
        max_pools=3,
    )

    scored = []
    counter = {"n": 0}

    @settings(
        max_examples=BUDGET,
        database=None,
        suppress_health_check=[HealthCheck.too_slow],
        derandomize=True,
    )
    @given(scenario=strat)
    def search(scenario):
        if counter["n"] >= BUDGET:
            return
        counter["n"] += 1
        metrics = evaluate(scenario)
        scored.append((metrics, scenario))
        if counter["n"] % 100 == 0:
            best = max(s[0]["score"] for s in scored)
            print(f"  evaluated {counter['n']}/{BUDGET}, best score: {best:.6f}")

    print(f"Running adversarial search (budget={BUDGET}, seeds={SEEDS})...")
    try:
        search()
    except Exception as e:
        if scored:
            print(f"  search stopped at {counter['n']} evaluations: {type(e).__name__}")
        else:
            raise

    # Group by category, rank separately, take top-k from each
    by_category = defaultdict(list)
    for metrics, scenario in scored:
        if metrics["score"] > 0:
            by_category[metrics["category"]].append((metrics, scenario))

    for cat in by_category:
        by_category[cat].sort(key=lambda x: x[0]["score"], reverse=True)

    os.makedirs(SCENARIO_DIR, exist_ok=True)
    os.makedirs(RESULTS_DIR, exist_ok=True)

    manifest_entries = []
    file_index = 0

    print(f"\nEvaluated {counter['n']} scenarios.\n")

    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        items = by_category.get(cat, [])[:TOP_K]
        if not items:
            continue
        print(f"  {cat}: {len(items)} scenarios (of {len(by_category.get(cat, []))} total)")
        for m, scenario in items:
            file_index += 1
            fname = f"worst_case_{file_index:02d}.yaml"
            path = os.path.join(SCENARIO_DIR, fname)
            with open(path, "w") as f:
                yaml.dump(strip_variants(scenario), f, default_flow_style=False, sort_keys=False)

            manifest_entries.append({
                "scenario": fname,
                "category": m["category"],
                "signed_delta": round(m["signed_delta"], 6),
                "score": round(m["score"], 6),
                "most_cost": round(m["most_cost"], 6),
                "least_cost": round(m["least_cost"], 6),
                "most_running": round(m["most_running"], 1),
                "least_running": round(m["least_running"], 1),
                "most_pending": round(m["most_pending"], 1),
                "least_pending": round(m["least_pending"], 1),
                "most_nodes": round(m["most_nodes"], 1),
                "least_nodes": round(m["least_nodes"], 1),
            })

    # Write manifest.json
    manifest = {
        "generated_by": "find_adversarial.py",
        "budget": BUDGET,
        "seeds": SEEDS,
        "top_k_per_category": TOP_K,
        "scenarios": manifest_entries,
    }
    manifest_path = os.path.join(RESULTS_DIR, "manifest.json")
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)

    # Write summary.md
    summary_lines = [
        "# Adversarial Finder Results\n",
        f"Budget: {BUDGET} scenarios, Seeds: {SEEDS}, Top-K per category: {TOP_K}\n",
        f"Total scenarios with divergence: {sum(len(v) for v in by_category.values())}\n",
    ]
    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        entries = [e for e in manifest_entries if e["category"] == cat]
        if not entries:
            continue
        summary_lines.append(f"\n## {cat}\n")
        summary_lines.append(f"| # | Scenario | Signed Δ | Most Cost | Least Cost |")
        summary_lines.append(f"|---|----------|----------|-----------|------------|")
        for i, e in enumerate(entries):
            summary_lines.append(
                f"| {i+1} | {e['scenario']} | {e['signed_delta']:+.4f} "
                f"| {e['most_cost']:.4f} | {e['least_cost']:.4f} |"
            )

    summary_path = os.path.join(RESULTS_DIR, "summary.md")
    with open(summary_path, "w") as f:
        f.write("\n".join(summary_lines) + "\n")

    print(f"\nScenario YAMLs: {os.path.abspath(SCENARIO_DIR)}/")
    print(f"Results manifest: {os.path.abspath(manifest_path)}")
    print(f"Results summary:  {os.path.abspath(summary_path)}")


if __name__ == "__main__":
    main()
