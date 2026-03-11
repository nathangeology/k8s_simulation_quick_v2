#!/usr/bin/env python3
"""Find adversarial scenarios where MostAllocated vs LeastAllocated diverges most.

Sweeps random cluster scenarios via Hypothesis strategies, injects both
scheduling variants, runs them through kubesim batch_run, and ranks by
signed divergence (direction-aware). Results are written to a separate
manifest so scenario YAMLs stay clean input files.

Categories:
  adversarial_to_most  — MostAllocated costs more
  adversarial_to_least — LeastAllocated costs more
  both_degrade         — both strategies degrade (mixed signals)
"""

import copy
import json
import os
import sys
from datetime import datetime, timezone

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
    """Categorize by which strategy is worse (higher cost)."""
    delta = most_cost - least_cost
    if delta > 0:
        return "adversarial_to_most"
    elif delta < 0:
        return "adversarial_to_least"
    return "both_degrade"


def evaluate(scenario: dict) -> dict:
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
    signed_delta = most_cost - least_cost
    category = categorize(most_cost, least_cost)

    return {
        "signed_delta": signed_delta,
        "abs_delta": abs(signed_delta),
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
    """Return a copy of the scenario with variants removed."""
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def write_manifest(entries: list[dict]) -> None:
    """Write results/adversarial/manifest.json."""
    os.makedirs(RESULTS_DIR, exist_ok=True)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "budget": BUDGET,
        "seeds": SEEDS,
        "top_k_per_category": TOP_K,
        "scenarios": entries,
    }
    path = os.path.join(RESULTS_DIR, "manifest.json")
    with open(path, "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"Manifest written to: {os.path.abspath(path)}")


def write_summary(entries: list[dict]) -> None:
    """Write results/adversarial/summary.md."""
    lines = [
        "# Adversarial Scenario Results",
        "",
        f"Generated: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}",
        f"Budget: {BUDGET} scenarios evaluated",
        "",
    ]

    by_cat = {}
    for e in entries:
        by_cat.setdefault(e["category"], []).append(e)

    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        group = by_cat.get(cat, [])
        label = {
            "adversarial_to_most": "MostAllocated costs more",
            "adversarial_to_least": "LeastAllocated costs more",
            "both_degrade": "Both degrade (mixed signals)",
        }[cat]
        lines.append(f"## {label} ({len(group)} scenarios)")
        lines.append("")
        if not group:
            lines.append("_No scenarios found in this category._")
            lines.append("")
            continue
        lines.append(f"{'File':<28} {'Δ cost/hr':>10} {'Most $/hr':>10} {'Least $/hr':>10}")
        lines.append("-" * 62)
        for e in group:
            lines.append(
                f"{e['filename']:<28} {e['signed_delta']:>+10.4f} "
                f"{e['most_cost']:>10.4f} {e['least_cost']:>10.4f}"
            )
        lines.append("")

    path = os.path.join(RESULTS_DIR, "summary.md")
    with open(path, "w") as f:
        f.write("\n".join(lines))
    print(f"Summary written to: {os.path.abspath(path)}")


def main():
    from hypothesis import HealthCheck, given, settings

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
                best = max(s[0]["abs_delta"] for s in scored)
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

    # Bucket by category, rank each separately by abs_delta, keep top-k per category
    by_cat = {}
    for metrics, scenario in scored:
        by_cat.setdefault(metrics["category"], []).append((metrics, scenario))

    for cat in by_cat:
        by_cat[cat].sort(key=lambda x: x[0]["abs_delta"], reverse=True)
        by_cat[cat] = by_cat[cat][:TOP_K]

    # Write clean scenario YAMLs and collect manifest entries
    os.makedirs(SCENARIO_DIR, exist_ok=True)
    # Remove old worst_case files
    for f in os.listdir(SCENARIO_DIR):
        if f.startswith("worst_case_") and f.endswith(".yaml"):
            os.remove(os.path.join(SCENARIO_DIR, f))

    manifest_entries = []
    file_idx = 0

    print(f"\nEvaluated {counter['n']} scenarios.")

    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        group = by_cat.get(cat, [])
        if not group:
            continue
        label = {
            "adversarial_to_most": "MostAllocated costs more",
            "adversarial_to_least": "LeastAllocated costs more",
            "both_degrade": "Both degrade",
        }[cat]
        print(f"\n{label} (top {len(group)}):")
        print(f"{'#':>3} {'Δ cost':>+10} {'Most $/hr':>10} {'Least $/hr':>10}  File")
        print("-" * 60)

        for i, (m, scenario) in enumerate(group):
            file_idx += 1
            fname = f"worst_case_{file_idx:02d}.yaml"
            path = os.path.join(SCENARIO_DIR, fname)
            with open(path, "w") as f:
                yaml.dump(strip_variants(scenario), f, default_flow_style=False, sort_keys=False)

            entry = {
                "filename": fname,
                "category": cat,
                "signed_delta": round(m["signed_delta"], 6),
                "abs_delta": round(m["abs_delta"], 6),
                "most_cost": round(m["most_cost"], 6),
                "least_cost": round(m["least_cost"], 6),
                "most_running": round(m["most_running"], 1),
                "least_running": round(m["least_running"], 1),
                "most_pending": round(m["most_pending"], 1),
                "least_pending": round(m["least_pending"], 1),
                "most_nodes": round(m["most_nodes"], 1),
                "least_nodes": round(m["least_nodes"], 1),
            }
            manifest_entries.append(entry)
            print(
                f"{i+1:3d} {m['signed_delta']:>+10.4f} {m['most_cost']:10.4f} "
                f"{m['least_cost']:10.4f}  {fname}"
            )

    write_manifest(manifest_entries)
    write_summary(manifest_entries)

    # Summary stats
    all_deltas = [s[0]["abs_delta"] for s in scored]
    nonzero = [d for d in all_deltas if d > 0]
    print(f"\nSummary:")
    print(f"  Total scenarios evaluated: {len(all_deltas)}")
    print(f"  Scenarios with divergence > 0: {len(nonzero)}")
    if nonzero:
        print(f"  Max abs delta: {max(nonzero):.6f}")
        print(f"  Mean abs delta (nonzero): {sum(nonzero)/len(nonzero):.6f}")

    cat_counts = {}
    for s in scored:
        cat_counts[s[0]["category"]] = cat_counts.get(s[0]["category"], 0) + 1
    for cat, count in sorted(cat_counts.items()):
        print(f"  {cat}: {count} scenarios")

    print(f"\nScenario YAMLs: {os.path.abspath(SCENARIO_DIR)}/")
    print(f"Results: {os.path.abspath(RESULTS_DIR)}/")


if __name__ == "__main__":
    main()
