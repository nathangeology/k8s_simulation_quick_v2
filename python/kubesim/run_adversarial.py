"""Run AdversarialFinder to discover worst-case MostAllocated vs LeastAllocated divergence.

Searches over ScenarioSpace with directional scoring (signed deltas) and
categorizes results as adversarial_to_most, adversarial_to_least, or both_degrade.

Output structure:
  scenarios/adversarial/*.yaml       — clean input YAMLs (no results)
  results/adversarial/manifest.json  — scores, direction, per-variant metrics
  results/adversarial/summary.md     — human-readable summary

Usage::

    python -m kubesim.run_adversarial [--budget 1000] [--top-k 10] [--outdir scenarios/adversarial]
"""

from __future__ import annotations

import argparse
import json
import copy
from collections import defaultdict
from pathlib import Path

import yaml

from kubesim.adversarial import AdversarialFinder, ScenarioSpace

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]


def _signed_divergence_metric(results: list[dict]) -> float:
    """Signed delta: most_cost - least_cost (positive = MostAllocated worse)."""
    most = [r["total_cost_per_hour"] for r in results if r["variant"] == "most_allocated"]
    least = [r["total_cost_per_hour"] for r in results if r["variant"] == "least_allocated"]
    if not most or not least:
        return 0.0
    return sum(most) / len(most) - sum(least) / len(least)


def _inject_variants(scenario: dict) -> dict:
    """Ensure scenario has both MostAllocated and LeastAllocated variants."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    return scenario


def _strip_variants(scenario: dict) -> dict:
    """Return a copy with variants/metrics removed (clean input)."""
    clean = copy.deepcopy(scenario)
    study = clean.get("study", clean)
    study.pop("variants", None)
    study.pop("metrics", None)
    return clean


def _categorize(signed_delta: float) -> str:
    if signed_delta > 0:
        return "adversarial_to_most"
    elif signed_delta < 0:
        return "adversarial_to_least"
    return "both_degrade"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Adversarial scenario finder: MostAllocated vs LeastAllocated")
    parser.add_argument("--budget", type=int, default=1000)
    parser.add_argument("--top-k", type=int, default=10)
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

    # Use abs(metric) for the finder's ranking, but we track signed values
    finder = AdversarialFinder(
        objective="maximize",
        metric=lambda results: abs(_signed_divergence_metric(results)),
        space=space,
        budget=args.budget,
        seeds=[42, 123, 7],
        top_k=args.budget,  # collect all, we'll categorize ourselves
        seed=args.seed,
    )

    _orig_evaluate = finder._evaluate

    def _patched_evaluate(scenario, rng):
        _inject_variants(scenario)
        return _orig_evaluate(scenario, rng)

    finder._evaluate = _patched_evaluate

    print(f"Running adversarial search: budget={args.budget}, space=nodes({space.nodes}), top_k={args.top_k}")
    ranked = finder.run()

    # Re-evaluate signed deltas and categorize
    by_category = defaultdict(list)
    for scored in ranked:
        _inject_variants(scored.scenario)
        config_yaml = yaml.dump(scored.scenario, default_flow_style=False)
        try:
            from kubesim._native import batch_run
            results = batch_run(config_yaml, [42, 123, 7])
            rows = [dict(r) if not isinstance(r, dict) else r for r in results]
        except Exception:
            continue
        signed = _signed_divergence_metric(rows)
        cat = _categorize(signed)
        by_category[cat].append((scored, signed, rows))

    for cat in by_category:
        by_category[cat].sort(key=lambda x: abs(x[1]), reverse=True)

    outdir = Path(args.outdir)
    results_dir = Path(args.results_dir)
    outdir.mkdir(parents=True, exist_ok=True)
    results_dir.mkdir(parents=True, exist_ok=True)

    manifest_entries = []
    file_index = 0

    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        items = by_category.get(cat, [])[:args.top_k]
        for scored, signed, rows in items:
            file_index += 1
            fname = f"worst_case_{file_index:03d}.yaml"
            path = outdir / fname
            with open(path, "w") as f:
                yaml.dump(_strip_variants(scored.scenario), f, default_flow_style=False, sort_keys=False)

            most = [r for r in rows if r["variant"] == "most_allocated"]
            least = [r for r in rows if r["variant"] == "least_allocated"]
            avg = lambda lst, k: sum(r[k] for r in lst) / len(lst) if lst else 0

            manifest_entries.append({
                "scenario": fname,
                "category": cat,
                "signed_delta": round(signed, 6),
                "score": round(abs(signed), 6),
                "most_cost": round(avg(most, "total_cost_per_hour"), 6),
                "least_cost": round(avg(least, "total_cost_per_hour"), 6),
            })
            print(f"  Saved {fname} ({cat}, delta={signed:+.4f})")

    manifest = {
        "generated_by": "run_adversarial.py",
        "budget": args.budget,
        "top_k_per_category": args.top_k,
        "scenarios": manifest_entries,
    }
    with open(results_dir / "manifest.json", "w") as f:
        json.dump(manifest, f, indent=2)

    summary_lines = [
        "# Adversarial Finder Results\n",
        f"Budget: {args.budget}, Top-K per category: {args.top_k}\n",
    ]
    for cat in ["adversarial_to_most", "adversarial_to_least", "both_degrade"]:
        entries = [e for e in manifest_entries if e["category"] == cat]
        if not entries:
            continue
        summary_lines.append(f"\n## {cat}\n")
        summary_lines.append("| # | Scenario | Signed Δ | Most Cost | Least Cost |")
        summary_lines.append("|---|----------|----------|-----------|------------|")
        for i, e in enumerate(entries):
            summary_lines.append(
                f"| {i+1} | {e['scenario']} | {e['signed_delta']:+.4f} "
                f"| {e['most_cost']:.4f} | {e['least_cost']:.4f} |"
            )

    with open(results_dir / "summary.md", "w") as f:
        f.write("\n".join(summary_lines) + "\n")

    print(f"\nScenario YAMLs: {outdir.resolve()}/")
    print(f"Results: {results_dir.resolve()}/")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
