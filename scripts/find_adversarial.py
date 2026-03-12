#!/usr/bin/env python3
"""Find adversarial scenarios where MostAllocated vs LeastAllocated diverges most.

Uses the expanded strategy space (chaos mode, single-instance pools, overcommit,
batch jobs with lifetimes, scale-down patterns) and formal objectives
(cost_efficiency, availability, scheduling_failure_rate, entropy_deviation).

Runs both normal and chaos searches, ranks by multi-objective divergence,
and saves top scenarios per category.
"""

import copy
import json
import os
import sys
from datetime import datetime, timezone

import yaml

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from kubesim._native import batch_run
from kubesim.strategies import cluster_scenario, chaos_scenario, ALL_WORKLOAD_TYPES
from kubesim.objectives import (
    cost_efficiency, availability, scheduling_failure_rate, entropy_deviation, OBJECTIVES,
)

BUDGET = 500
TOP_K = 10
SEEDS = [42, 100, 200]
BASE_DIR = os.path.join(os.path.dirname(__file__), "..")
SCENARIO_DIR = os.path.join(BASE_DIR, "scenarios", "adversarial")
RESULTS_DIR = os.path.join(BASE_DIR, "results", "adversarial")

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]

OBJECTIVE_FNS = {
    "cost_efficiency": cost_efficiency,
    "availability": availability,
    "scheduling_failure_rate": scheduling_failure_rate,
    "entropy_deviation": entropy_deviation,
}


def evaluate(scenario: dict) -> dict | None:
    """Run scenario with both variants, return multi-objective divergence metrics."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    config_yaml = yaml.dump(scenario, default_flow_style=False)
    try:
        results = batch_run(config_yaml, SEEDS)
    except Exception:
        return None

    rows = [dict(r) if not isinstance(r, dict) else r for r in results]
    most = [r for r in rows if r.get("variant") == "most_allocated"]
    least = [r for r in rows if r.get("variant") == "least_allocated"]
    if not most or not least:
        return None

    def avg(lst, key):
        return sum(r.get(key, 0) for r in lst) / len(lst)

    most_cost = avg(most, "total_cost_per_hour")
    least_cost = avg(least, "total_cost_per_hour")
    signed_delta = most_cost - least_cost

    # Compute per-objective scores for each variant
    obj_scores = {}
    for name, fn in OBJECTIVE_FNS.items():
        m_score = fn(most)
        l_score = fn(least)
        delta = m_score - l_score
        # Skip infinite/nan deltas (e.g. cost_efficiency with 0 running pods)
        if not (abs(delta) < float("inf")):
            delta = 0.0
        obj_scores[name] = {"most": m_score, "least": l_score, "delta": delta}

    # Combined divergence: sum of absolute deltas across all objectives
    combined_divergence = sum(abs(v["delta"]) for v in obj_scores.values())

    if signed_delta > 0:
        category = "adversarial_to_most"
    elif signed_delta < 0:
        category = "adversarial_to_least"
    else:
        category = "both_degrade"

    return {
        "signed_delta": signed_delta,
        "abs_delta": abs(signed_delta),
        "combined_divergence": combined_divergence,
        "category": category,
        "most_cost": most_cost,
        "least_cost": least_cost,
        "most_running": avg(most, "running_pods"),
        "least_running": avg(least, "running_pods"),
        "most_pending": avg(most, "pending_pods"),
        "least_pending": avg(least, "pending_pods"),
        "most_nodes": avg(most, "node_count"),
        "least_nodes": avg(least, "node_count"),
        "objectives": obj_scores,
    }


def strip_variants(scenario: dict) -> dict:
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def run_search(strat, budget, label):
    """Run hypothesis search with given strategy, return scored list."""
    from hypothesis import HealthCheck, given, settings

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
        metrics = evaluate(scenario)
        if metrics:
            scored.append((metrics, scenario))
        if counter["n"] % 100 == 0:
            best = max((s[0]["combined_divergence"] for s in scored), default=0)
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


def write_manifest(entries: list[dict]) -> None:
    os.makedirs(RESULTS_DIR, exist_ok=True)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "budget": BUDGET,
        "seeds": SEEDS,
        "top_k_per_category": TOP_K,
        "objectives": list(OBJECTIVE_FNS.keys()),
        "scenarios": entries,
    }
    path = os.path.join(RESULTS_DIR, "manifest.json")
    with open(path, "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"Manifest: {os.path.abspath(path)}")


def write_summary(manifest_entries: list[dict], total: int) -> None:
    """Write human-readable summary.md alongside the manifest."""
    os.makedirs(RESULTS_DIR, exist_ok=True)
    lines = [
        "# Adversarial Scenario Results\n",
        f"Generated: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M')} UTC",
        f"Budget: {BUDGET} scenarios evaluated per search mode\n",
    ]

    cat_labels = {
        "adversarial_to_most": "MostAllocated costs more",
        "adversarial_to_least": "LeastAllocated costs more",
        "both_degrade": "Both degrade (mixed signals)",
    }

    for cat, label in cat_labels.items():
        group = [e for e in manifest_entries if e["category"] == cat]
        lines.append(f"\n## {label} ({len(group)} scenarios)\n")
        if not group:
            lines.append("_No scenarios found in this category._\n")
            continue
        lines.append(f"{'File':<30} {'Δ cost/hr':>10} {'Combined':>10} {'Most $/hr':>10} {'Least $/hr':>10}")
        lines.append("-" * 72)
        for e in group:
            lines.append(
                f"{e['filename']:<30} {e['signed_delta']:>+10.4f} {e['combined_divergence']:>10.4f} "
                f"{e['most_cost']:10.4f} {e['least_cost']:10.4f}"
            )

    # Stats
    divs = [e["combined_divergence"] for e in manifest_entries]
    nonzero = [d for d in divs if d > 0]
    lines.append(f"\n## Summary\n")
    lines.append(f"- Total scenarios saved: {len(manifest_entries)}")
    lines.append(f"- Total evaluated: {total}")
    if nonzero:
        lines.append(f"- Max combined divergence: {max(nonzero):.6f}")
        lines.append(f"- Mean divergence (nonzero): {sum(nonzero)/len(nonzero):.6f}")

    path = os.path.join(RESULTS_DIR, "summary.md")
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"Summary: {os.path.abspath(path)}")


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Adversarial scenario finder (multi-objective)")
    parser.add_argument("--budget", type=int, default=BUDGET)
    parser.add_argument("--top-k", type=int, default=TOP_K)
    args = parser.parse_args()

    budget = args.budget
    top_k = args.top_k

    # Run both normal (expanded workloads) and chaos searches
    normal_strat = cluster_scenario(
        max_nodes=200,
        workload_types=ALL_WORKLOAD_TYPES,
        min_workloads=2,
        max_workloads=8,
        min_pools=1,
        max_pools=3,
    )
    chaos_strat = chaos_scenario(max_nodes=200)

    normal_scored = run_search(normal_strat, budget, "normal+expanded")
    chaos_scored = run_search(chaos_strat, budget, "chaos")

    all_scored = normal_scored + chaos_scored
    print(f"\nTotal evaluated: {len(all_scored)} scenarios")

    # Bucket by category, rank by combined_divergence
    by_cat: dict[str, list] = {}
    for metrics, scenario in all_scored:
        by_cat.setdefault(metrics["category"], []).append((metrics, scenario))

    for cat in by_cat:
        by_cat[cat].sort(key=lambda x: x[0]["combined_divergence"], reverse=True)
        by_cat[cat] = by_cat[cat][:top_k]

    # Write scenario YAMLs
    os.makedirs(SCENARIO_DIR, exist_ok=True)
    for f in os.listdir(SCENARIO_DIR):
        if f.startswith("worst_case_") and f.endswith(".yaml"):
            os.remove(os.path.join(SCENARIO_DIR, f))

    manifest_entries = []
    file_idx = 0

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
        print(f"{'#':>3} {'Δ cost':>10} {'Combined':>10} {'Most $/hr':>10} {'Least $/hr':>10}  File")
        print("-" * 70)

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
                "combined_divergence": round(m["combined_divergence"], 6),
                "most_cost": round(m["most_cost"], 6),
                "least_cost": round(m["least_cost"], 6),
                "most_running": round(m["most_running"], 1),
                "least_running": round(m["least_running"], 1),
                "most_pending": round(m["most_pending"], 1),
                "least_pending": round(m["least_pending"], 1),
                "most_nodes": round(m["most_nodes"], 1),
                "least_nodes": round(m["least_nodes"], 1),
                "objectives": {
                    k: {kk: round(vv, 6) if abs(vv) < float("inf") else None for kk, vv in v.items()}
                    for k, v in m["objectives"].items()
                },
            }
            manifest_entries.append(entry)
            print(
                f"{i+1:3d} {m['signed_delta']:>+10.4f} {m['combined_divergence']:>10.4f} "
                f"{m['most_cost']:10.4f} {m['least_cost']:10.4f}  {fname}"
            )

    write_manifest(manifest_entries)
    write_summary(manifest_entries, len(all_scored))

    print(f"\nScenarios: {os.path.abspath(SCENARIO_DIR)}/")
    print(f"Results: {os.path.abspath(RESULTS_DIR)}/")


if __name__ == "__main__":
    main()
