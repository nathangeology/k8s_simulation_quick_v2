#!/usr/bin/env python3
"""Adversarial discovery: Karpenter v0.35 vs v1.x.

Uses Optuna TPE (Bayesian optimization) to find scenarios where the two
Karpenter versions diverge most across cost_efficiency, availability,
and consolidation_waste objectives.
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
from kubesim.adversarial import OptunaAdversarialSearch, VariantPair
from kubesim.objectives import cost_efficiency, availability, consolidation_waste
from kubesim.strategies import ALL_WORKLOAD_TYPES
from kubesim.report import run_report

BASE_DIR = Path(__file__).resolve().parent.parent
SCENARIO_DIR = BASE_DIR / "scenarios" / "adversarial" / "karpenter-version"
RESULTS_DIR = BASE_DIR / "results" / "adversarial" / "karpenter-version"

KARPENTER_VERSION_PAIR = VariantPair(
    name_a="karpenter-v0.35",
    config_a={"name": "karpenter-v0.35", "karpenter_version": "v0.35"},
    name_b="karpenter-v1.x",
    config_b={"name": "karpenter-v1.x", "karpenter_version": "v1"},
)

SEEDS = [42, 100, 200]
OBJECTIVE_FNS = {
    "cost_efficiency": cost_efficiency,
    "availability": availability,
    "consolidation_waste": consolidation_waste,
}


def _divergence_metric(results: list[dict]) -> float:
    """Multi-objective divergence between the two karpenter versions."""
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


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Adversarial finder: Karpenter v0.35 vs v1.x (Optuna TPE)")
    parser.add_argument("--budget", type=int, default=500)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--report-seeds", type=int, default=50)
    parser.add_argument("--report-top", type=int, default=5)
    args = parser.parse_args()

    # Normal search
    print(f"Running normal Optuna search (budget={args.budget})...")
    normal_search = OptunaAdversarialSearch(
        objective_fn=_divergence_metric,
        seeds=SEEDS,
        budget=args.budget,
        top_k=args.top_k,
        workload_types=ALL_WORKLOAD_TYPES,
        max_pools=3,
        max_nodes=200,
        variant_pair=KARPENTER_VERSION_PAIR,
        chaos=False,
    )
    normal_ranked = normal_search.run()
    print(f"  Normal: {len(normal_ranked)} top scenarios")

    # Chaos search
    print(f"Running chaos Optuna search (budget={args.budget})...")
    chaos_search = OptunaAdversarialSearch(
        objective_fn=_divergence_metric,
        seeds=SEEDS,
        budget=args.budget,
        top_k=args.top_k,
        workload_types=ALL_WORKLOAD_TYPES,
        max_pools=3,
        max_nodes=200,
        variant_pair=KARPENTER_VERSION_PAIR,
        chaos=True,
    )
    chaos_ranked = chaos_search.run()
    print(f"  Chaos: {len(chaos_ranked)} top scenarios")

    # Merge and re-rank
    all_ranked = normal_ranked + chaos_ranked
    all_ranked.sort(key=lambda s: s.score, reverse=True)
    top = all_ranked[:args.top_k]

    # Save scenarios
    SCENARIO_DIR.mkdir(parents=True, exist_ok=True)
    for f in SCENARIO_DIR.iterdir():
        if f.name.startswith("worst_case_") and f.suffix == ".yaml":
            f.unlink()

    print(f"\nTop {len(top)} scenarios by combined divergence:")
    print(f"{'#':>3} {'Score':>10}  File")
    print("-" * 40)

    manifest_entries = []
    for i, scored in enumerate(top):
        fname = f"worst_case_{i+1:02d}.yaml"
        path = SCENARIO_DIR / fname
        scenario = copy.deepcopy(scored.scenario)
        study = scenario.get("study", scenario)
        study["variants"] = [KARPENTER_VERSION_PAIR.config_a, KARPENTER_VERSION_PAIR.config_b]
        study["scheduling_strategy"] = "full_scan"
        with open(path, "w") as f:
            f.write(f"# Adversarial scenario #{i+1} — divergence: {scored.score:.4f}\n")
            f.write(f"# Karpenter v0.35 vs v1.x (Optuna TPE)\n")
            yaml.dump(scenario, f, default_flow_style=False, sort_keys=False)

        manifest_entries.append({
            "filename": fname,
            "divergence_score": round(scored.score, 6),
        })
        print(f"{i+1:3d} {scored.score:>10.4f}  {fname}")

    # Write manifest
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "search_method": "optuna_tpe",
        "budget": args.budget,
        "seeds": SEEDS,
        "top_k": args.top_k,
        "objectives": list(OBJECTIVE_FNS.keys()),
        "variant_a": "karpenter-v0.35",
        "variant_b": "karpenter-v1.x",
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
        scenario_path = SCENARIO_DIR / f"worst_case_{i+1:02d}.yaml"
        out_dir = str(RESULTS_DIR)
        print(f"  Report {i+1}/{report_n}: {scenario_path.name}")
        try:
            run_report(str(scenario_path), seeds=args.report_seeds, output_dir=out_dir)
        except Exception as e:
            print(f"    WARNING: report failed: {e}")

    print(f"\nScenarios: {SCENARIO_DIR}/")
    print(f"Results: {RESULTS_DIR}/")


if __name__ == "__main__":
    main()
