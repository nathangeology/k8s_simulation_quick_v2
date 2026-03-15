#!/usr/bin/env python3
"""Find adversarial scenarios where MostAllocated vs LeastAllocated diverges most.

Uses Optuna TPE (Bayesian optimization) to guide the search over the scenario
space. Evaluates multi-objective divergence across cost_efficiency, availability,
scheduling_failure_rate, and entropy_deviation.

Runs both normal and chaos searches, ranks by combined divergence,
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
from kubesim.adversarial import OptunaAdversarialSearch, VariantPair, ScoredScenario
from kubesim.strategies import ALL_WORKLOAD_TYPES
from kubesim.objectives import (
    cost_efficiency, availability, scheduling_failure_rate, entropy_deviation, OBJECTIVES,
)

BUDGET = 100          # was 500 — dial back to avoid OOM crashes, increase gradually
TOP_K = 10
SEEDS = [42, 100, 200]
SEARCH_SEEDS = [42]   # single seed during search for speed; full SEEDS for final re-eval
BASE_DIR = os.path.join(os.path.dirname(__file__), "..")
SCENARIO_DIR = os.path.join(BASE_DIR, "scenarios", "adversarial", "scheduling")
RESULTS_DIR = os.path.join(BASE_DIR, "results", "adversarial")

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]

VARIANT_PAIR = VariantPair(
    name_a="most_allocated",
    config_a=VARIANTS[0],
    name_b="least_allocated",
    config_b=VARIANTS[1],
)

OBJECTIVE_FNS = {
    "cost_efficiency": cost_efficiency,
    "availability": availability,
    "scheduling_failure_rate": scheduling_failure_rate,
    "entropy_deviation": entropy_deviation,
}


def _combined_divergence(results: list[dict]) -> float:
    """Multi-objective divergence between the two scheduling variants."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)
    if len(by_variant) < 2:
        return 0.0
    groups = list(by_variant.values())
    total = 0.0
    for fn in OBJECTIVE_FNS.values():
        a, b = fn(groups[0]), fn(groups[1])
        delta = abs(a - b)
        if delta < float("inf"):
            total += delta
    return total


def _categorize(scenario: dict, results: list[dict]) -> dict | None:
    """Evaluate a scenario and return detailed metrics with category."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r)
    most = by_variant.get("most_allocated", [])
    least = by_variant.get("least_allocated", [])
    if not most or not least:
        return None

    def avg(lst, key):
        return sum(r.get(key, 0) for r in lst) / len(lst)

    most_cost = avg(most, "total_cost_per_hour")
    least_cost = avg(least, "total_cost_per_hour")
    signed_delta = most_cost - least_cost

    obj_scores = {}
    for name, fn in OBJECTIVE_FNS.items():
        m_score, l_score = fn(most), fn(least)
        delta = m_score - l_score
        if not (abs(delta) < float("inf")):
            delta = 0.0
        obj_scores[name] = {"most": m_score, "least": l_score, "delta": delta}

    combined = sum(abs(v["delta"]) for v in obj_scores.values())

    if signed_delta > 0:
        category = "adversarial_to_most"
    elif signed_delta < 0:
        category = "adversarial_to_least"
    else:
        category = "both_degrade"

    return {
        "signed_delta": signed_delta,
        "abs_delta": abs(signed_delta),
        "combined_divergence": combined,
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


def write_manifest(entries: list[dict]) -> None:
    os.makedirs(RESULTS_DIR, exist_ok=True)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "search_method": "optuna_tpe",
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


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Adversarial scenario finder (Optuna TPE)")
    parser.add_argument("--budget", type=int, default=BUDGET)
    parser.add_argument("--top-k", type=int, default=TOP_K)
    args = parser.parse_args()

    budget = args.budget
    top_k = args.top_k

    # Normal search — use single seed during search, full seeds for final re-eval
    print(f"Running normal Optuna search (budget={budget})...")
    normal_search = OptunaAdversarialSearch(
        objective_fn=_combined_divergence,
        seeds=SEARCH_SEEDS,
        budget=budget,
        top_k=min(50, budget),  # cap re-eval pool to avoid OOM
        workload_types=ALL_WORKLOAD_TYPES,
        max_pools=3,
        max_nodes=50,           # was 200 — dial back, increase gradually
        variant_pair=VARIANT_PAIR,
        chaos=False,
    )
    normal_ranked = normal_search.run()
    print(f"  Normal: {len(normal_ranked)} scenarios evaluated")

    # Chaos search
    print(f"Running chaos Optuna search (budget={budget})...")
    chaos_search = OptunaAdversarialSearch(
        objective_fn=_combined_divergence,
        seeds=SEARCH_SEEDS,
        budget=budget,
        top_k=min(50, budget),
        workload_types=ALL_WORKLOAD_TYPES,
        max_pools=3,
        max_nodes=50,
        variant_pair=VARIANT_PAIR,
        chaos=True,
    )
    chaos_ranked = chaos_search.run()
    print(f"  Chaos: {len(chaos_ranked)} scenarios evaluated")

    # Re-evaluate top scenarios for detailed categorization
    # Only re-eval top 30 by score (not all) to limit memory
    all_scored = normal_ranked + chaos_ranked
    all_scored.sort(key=lambda s: s.score, reverse=True)
    top_for_reeval = all_scored[:30]
    print(f"\nTotal evaluated: {len(all_scored)} scenarios, re-evaluating top {len(top_for_reeval)} with full seeds")

    # Re-run top scenarios with full SEEDS to get per-variant metrics
    categorized: list[tuple[dict, dict]] = []
    for scored in top_for_reeval:
        scenario = copy.deepcopy(scored.scenario)
        study = scenario.get("study", scenario)
        study["variants"] = VARIANTS
        study["scheduling_strategy"] = "reverse_schedule"
        config_yaml = yaml.dump(scenario, default_flow_style=False)
        try:
            raw = batch_run(config_yaml, SEEDS)
            results = [dict(r) if not isinstance(r, dict) else r for r in raw]
        except Exception:
            continue
        metrics = _categorize(scenario, results)
        if metrics:
            categorized.append((metrics, scored.scenario))

    # Bucket by category, rank by combined_divergence
    by_cat: dict[str, list] = {}
    for metrics, scenario in categorized:
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
            clean = strip_variants(scenario)
            # Set full_scan for validation
            study = clean.get("study", clean)
            study["scheduling_strategy"] = "full_scan"
            with open(path, "w") as f:
                yaml.dump(clean, f, default_flow_style=False, sort_keys=False)

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

    # Summary
    all_divs = [s[0]["combined_divergence"] for s in categorized]
    nonzero = [d for d in all_divs if d > 0]
    print(f"\nSummary:")
    print(f"  Search method: Optuna TPE (Bayesian)")
    print(f"  Total scenarios: {len(all_divs)}")
    print(f"  With divergence > 0: {len(nonzero)}")
    if nonzero:
        print(f"  Max combined divergence: {max(nonzero):.6f}")
        print(f"  Mean (nonzero): {sum(nonzero)/len(nonzero):.6f}")

    cat_counts = {}
    for m, _ in categorized:
        cat_counts[m["category"]] = cat_counts.get(m["category"], 0) + 1
    for cat, count in sorted(cat_counts.items()):
        print(f"  {cat}: {count}")

    print(f"\nScenarios: {os.path.abspath(SCENARIO_DIR)}/")
    print(f"Results: {os.path.abspath(RESULTS_DIR)}/")


if __name__ == "__main__":
    main()
