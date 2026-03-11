"""Run AdversarialFinder to discover worst-case MostAllocated vs LeastAllocated divergence.

Searches over ScenarioSpace with directional scoring (signed deltas).
Categorizes results and writes clean input YAMLs + separate results manifest.

Usage::

    python -m kubesim.run_adversarial [--budget 1000] [--top-k 5] [--outdir scenarios/adversarial]
"""

from __future__ import annotations

import argparse
import copy
import json
from datetime import datetime, timezone
from pathlib import Path

import yaml

from kubesim.adversarial import AdversarialFinder, ScenarioSpace

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]


def _signed_divergence_metric(results: list[dict]) -> float:
    """Signed cost delta: most_cost - least_cost. Positive = MostAllocated worse."""
    most = [r["total_cost_per_hour"] for r in results if r["variant"] == "most_allocated"]
    least = [r["total_cost_per_hour"] for r in results if r["variant"] == "least_allocated"]
    if not most or not least:
        return 0.0
    return sum(most) / len(most) - sum(least) / len(least)


def _inject_variants(scenario: dict) -> dict:
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    return scenario


def _strip_variants(scenario: dict) -> dict:
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Adversarial scenario finder: MostAllocated vs LeastAllocated")
    parser.add_argument("--budget", type=int, default=1000)
    parser.add_argument("--top-k", type=int, default=5)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--outdir", type=str, default="scenarios/adversarial")
    parser.add_argument("--results-dir", type=str, default="results/adversarial")
    args = parser.parse_args(argv)

    space = ScenarioSpace(
        nodes=(10, 1000),
        workload_types=["web_app", "ml_training", "batch_job", "saas_microservice"],
        min_workloads=2,
        max_workloads=8,
        traffic=None,
        min_pools=1,
        max_pools=3,
    )

    # We run the finder twice: once maximizing (adversarial_to_most) and once
    # minimizing (adversarial_to_least) the signed delta.
    seeds = [42, 123, 7]
    categories = {}

    for objective, cat_name in [("maximize", "adversarial_to_most"), ("minimize", "adversarial_to_least")]:
        finder = AdversarialFinder(
            objective=objective,
            metric=_signed_divergence_metric,
            space=space,
            budget=args.budget,
            seeds=seeds,
            top_k=args.top_k,
            seed=args.seed,
        )
        _orig_evaluate = finder._evaluate
        def _patched(scenario, rng, _orig=_orig_evaluate):
            _inject_variants(scenario)
            return _orig(scenario, rng)
        finder._evaluate = _patched

        print(f"Searching {cat_name} (objective={objective}, budget={args.budget})...")
        ranked = finder.run()
        categories[cat_name] = ranked

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    results_dir = Path(args.results_dir)
    results_dir.mkdir(parents=True, exist_ok=True)

    manifest_entries = []
    file_index = 0

    for cat_name, ranked in categories.items():
        for i, scored in enumerate(ranked):
            file_index += 1
            fname = f"worst_case_{file_index:02d}.yaml"
            path = outdir / fname

            clean = _strip_variants(scored.scenario)
            with open(path, "w") as f:
                yaml.dump(clean, f, default_flow_style=False, sort_keys=False)

            # Re-evaluate to get full per-variant metrics
            _inject_variants(scored.scenario)
            manifest_entries.append({
                "scenario_file": fname,
                "category": cat_name,
                "rank_in_category": i + 1,
                "cost_delta": round(scored.score, 6),
                "abs_cost_delta": round(abs(scored.score), 6),
            })
            print(f"  Saved {fname} ({cat_name}, delta={scored.score:+.4f})")

    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "budget": args.budget,
        "seeds": seeds,
        "top_k_per_category": args.top_k,
        "scenarios": manifest_entries,
    }

    with open(results_dir / "manifest.json", "w") as f:
        json.dump(manifest, f, indent=2)

    with open(results_dir / "summary.md", "w") as f:
        f.write("# Adversarial Finder Results\n\n")
        f.write(f"Generated: {manifest['generated_at']}\n\n")
        f.write(f"- Budget: {args.budget}\n- Seeds: {seeds}\n- Top-k per category: {args.top_k}\n\n")
        for cat_name in ["adversarial_to_most", "adversarial_to_least"]:
            entries = [e for e in manifest_entries if e["category"] == cat_name]
            f.write(f"## {cat_name} ({len(entries)} scenarios)\n\n")
            if not entries:
                f.write("No scenarios found.\n\n")
                continue
            f.write("| # | File | CostΔ |\n|---|------|-------|\n")
            for e in entries:
                f.write(f"| {e['rank_in_category']} | {e['scenario_file']} | {e['cost_delta']:+.4f} |\n")
            f.write("\n")

    print(f"\nScenarios: {outdir.resolve()}/")
    print(f"Manifest: {(results_dir / 'manifest.json').resolve()}")
    print(f"Summary: {(results_dir / 'summary.md').resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
