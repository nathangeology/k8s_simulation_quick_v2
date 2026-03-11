#!/usr/bin/env python3
"""Find adversarial scenarios where MostAllocated vs LeastAllocated diverges most.

Sweeps random cluster scenarios via Hypothesis strategies, injects both
scheduling variants, runs them through kubesim batch_run, and ranks by
signed (directional) divergence.

Scenarios are categorized by direction:
- adversarial_to_most: MostAllocated costs more
- adversarial_to_least: LeastAllocated costs more
- both_degrade: both strategies degrade (neither clearly wins)

Output:
- scenarios/adversarial/*.yaml — clean input YAMLs (no results data)
- results/adversarial/manifest.json — scores, direction, per-variant metrics
- results/adversarial/summary.md — human-readable summary
"""

import json
import os
import sys
from datetime import datetime, timezone

import yaml

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from kubesim._native import batch_run
from kubesim.strategies import cluster_scenario

BUDGET = 1000
TOP_K = 5
SEEDS = [42, 100, 200]
BASE_DIR = os.path.join(os.path.dirname(__file__), "..")
SCENARIO_DIR = os.path.join(BASE_DIR, "scenarios", "adversarial")
RESULTS_DIR = os.path.join(BASE_DIR, "results", "adversarial")

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]


def evaluate(scenario: dict) -> dict | None:
    """Run scenario with both variants, return directional metrics."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    config_yaml = yaml.dump(scenario, default_flow_style=False)
    try:
        results = batch_run(config_yaml, SEEDS)
    except Exception:
        return None

    rows = [dict(r) if not isinstance(r, dict) else r for r in results]
    most = [r for r in rows if r["variant"] == "most_allocated"]
    least = [r for r in rows if r["variant"] == "least_allocated"]
    if not most or not least:
        return None

    def avg(lst, key):
        return sum(r[key] for r in lst) / len(lst)

    most_cost = avg(most, "total_cost_per_hour")
    least_cost = avg(least, "total_cost_per_hour")

    # Signed delta: positive means MostAllocated is worse (costs more)
    cost_delta = most_cost - least_cost

    if cost_delta > 0:
        category = "adversarial_to_most"
    elif cost_delta < 0:
        category = "adversarial_to_least"
    else:
        category = "both_degrade"

    return {
        "cost_delta": cost_delta,
        "abs_cost_delta": abs(cost_delta),
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


def categorize_and_rank(scored: list[tuple[dict, dict]], top_k: int) -> dict[str, list]:
    """Rank separately per category, keep top_k from each."""
    buckets: dict[str, list] = {
        "adversarial_to_most": [],
        "adversarial_to_least": [],
        "both_degrade": [],
    }
    for metrics, scenario in scored:
        buckets[metrics["category"]].append((metrics, scenario))

    ranked = {}
    for cat, items in buckets.items():
        items.sort(key=lambda x: x[0]["abs_cost_delta"], reverse=True)
        ranked[cat] = items[:top_k]
    return ranked


def strip_variants(scenario: dict) -> dict:
    """Return a copy of scenario with variants removed (clean input)."""
    import copy
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def write_outputs(ranked: dict[str, list], total_evaluated: int):
    """Write clean scenario YAMLs, manifest.json, and summary.md."""
    os.makedirs(SCENARIO_DIR, exist_ok=True)
    os.makedirs(RESULTS_DIR, exist_ok=True)

    manifest_entries = []
    file_index = 0

    for cat, items in ranked.items():
        for i, (metrics, scenario) in enumerate(items):
            file_index += 1
            fname = f"worst_case_{file_index:02d}.yaml"
            path = os.path.join(SCENARIO_DIR, fname)

            clean = strip_variants(scenario)
            with open(path, "w") as f:
                yaml.dump(clean, f, default_flow_style=False, sort_keys=False)

            manifest_entries.append({
                "scenario_file": fname,
                "category": cat,
                "rank_in_category": i + 1,
                "cost_delta": round(metrics["cost_delta"], 6),
                "abs_cost_delta": round(metrics["abs_cost_delta"], 6),
                "most_allocated": {
                    "cost_per_hour": round(metrics["most_cost"], 6),
                    "running_pods": round(metrics["most_running"], 1),
                    "pending_pods": round(metrics["most_pending"], 1),
                    "node_count": round(metrics["most_nodes"], 1),
                },
                "least_allocated": {
                    "cost_per_hour": round(metrics["least_cost"], 6),
                    "running_pods": round(metrics["least_running"], 1),
                    "pending_pods": round(metrics["least_pending"], 1),
                    "node_count": round(metrics["least_nodes"], 1),
                },
            })

    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "budget": BUDGET,
        "seeds": SEEDS,
        "top_k_per_category": TOP_K,
        "total_evaluated": total_evaluated,
        "scenarios": manifest_entries,
    }

    manifest_path = os.path.join(RESULTS_DIR, "manifest.json")
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)

    # Write summary.md
    summary_path = os.path.join(RESULTS_DIR, "summary.md")
    with open(summary_path, "w") as f:
        f.write("# Adversarial Finder Results\n\n")
        f.write(f"Generated: {manifest['generated_at']}\n\n")
        f.write(f"- Budget: {BUDGET} scenarios evaluated ({total_evaluated} completed)\n")
        f.write(f"- Seeds: {SEEDS}\n")
        f.write(f"- Top-k per category: {TOP_K}\n\n")

        for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
            entries = [e for e in manifest_entries if e["category"] == cat]
            f.write(f"## {cat} ({len(entries)} scenarios)\n\n")
            if not entries:
                f.write("No scenarios found in this category.\n\n")
                continue
            f.write(f"| # | File | CostΔ | Most $/hr | Least $/hr |\n")
            f.write(f"|---|------|-------|-----------|------------|\n")
            for e in entries:
                f.write(f"| {e['rank_in_category']} | {e['scenario_file']} "
                        f"| {e['cost_delta']:+.4f} "
                        f"| {e['most_allocated']['cost_per_hour']:.4f} "
                        f"| {e['least_allocated']['cost_per_hour']:.4f} |\n")
            f.write("\n")

    return manifest_path, summary_path


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
        if metrics:
            scored.append((metrics, scenario))
        if counter["n"] % 100 == 0:
            if scored:
                best = max(s[0]["abs_cost_delta"] for s in scored)
                print(f"  evaluated {counter['n']}/{BUDGET}, best abs delta: {best:.6f}")
            else:
                print(f"  evaluated {counter['n']}/{BUDGET}, no valid results yet")

    print(f"Running adversarial search (budget={BUDGET}, seeds={SEEDS})...")
    try:
        search()
    except Exception as e:
        if scored:
            print(f"  search stopped at {counter['n']} evaluations: {type(e).__name__}")
        else:
            raise

    ranked = categorize_and_rank(scored, TOP_K)

    # Remove old scenario files
    if os.path.isdir(SCENARIO_DIR):
        for f in os.listdir(SCENARIO_DIR):
            if f.startswith("worst_case_") and f.endswith(".yaml"):
                os.remove(os.path.join(SCENARIO_DIR, f))

    manifest_path, summary_path = write_outputs(ranked, counter["n"])

    # Print summary to stdout
    print(f"\nEvaluated {counter['n']} scenarios ({len(scored)} valid).\n")
    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        items = ranked.get(cat, [])
        print(f"  {cat}: {len(items)} scenarios saved")
        for m, _ in items:
            print(f"    cost_delta={m['cost_delta']:+.4f}  most={m['most_cost']:.4f}  least={m['least_cost']:.4f}")

    print(f"\nScenario YAMLs: {os.path.abspath(SCENARIO_DIR)}/")
    print(f"Results manifest: {os.path.abspath(manifest_path)}")
    print(f"Summary: {os.path.abspath(summary_path)}")


if __name__ == "__main__":
    main()
