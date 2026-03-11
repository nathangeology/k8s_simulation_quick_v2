"""Run AdversarialFinder to discover worst-case MostAllocated vs LeastAllocated divergence.

Searches over ScenarioSpace(nodes=(10,1000), mixed workloads, varying churn)
with budget=1000. Saves discovered worst-cases as YAML in scenarios/adversarial/.

Usage::

    python -m kubesim.run_adversarial [--budget 1000] [--top-k 10] [--outdir scenarios/adversarial]
"""

from __future__ import annotations

import argparse
from pathlib import Path

import yaml

from kubesim.adversarial import AdversarialFinder, ScenarioSpace


def _divergence_metric(results: list[dict]) -> float:
    """Absolute difference in mean total_cost_per_hour between MostAllocated and LeastAllocated."""
    most = [r["total_cost_per_hour"] for r in results if r["variant"] == "most_allocated"]
    least = [r["total_cost_per_hour"] for r in results if r["variant"] == "least_allocated"]
    if not most or not least:
        return 0.0
    return abs(sum(most) / len(most) - sum(least) / len(least))


def _inject_variants(scenario: dict) -> dict:
    """Ensure scenario has both MostAllocated and LeastAllocated variants."""
    study = scenario.get("study", scenario)
    study["variants"] = [
        {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
        {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
    ]
    return scenario


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Adversarial scenario finder: MostAllocated vs LeastAllocated")
    parser.add_argument("--budget", type=int, default=1000)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--outdir", type=str, default="scenarios/adversarial")
    args = parser.parse_args(argv)

    space = ScenarioSpace(
        nodes=(10, 1000),
        workload_types=["web_app", "ml_training", "batch_job", "saas_microservice"],
        min_workloads=2,
        max_workloads=8,
        traffic=None,  # varying — random on/off
        min_pools=1,
        max_pools=3,
    )

    finder = AdversarialFinder(
        objective="maximize",
        metric=_divergence_metric,
        space=space,
        budget=args.budget,
        seeds=[42, 123, 7],  # multiple seeds for robustness
        top_k=args.top_k,
        seed=args.seed,
    )

    # Monkey-patch _evaluate to inject both variants before running
    _orig_evaluate = finder._evaluate

    def _patched_evaluate(scenario, rng):
        _inject_variants(scenario)
        return _orig_evaluate(scenario, rng)

    finder._evaluate = _patched_evaluate

    print(f"Running adversarial search: budget={args.budget}, space=nodes({space.nodes}), top_k={args.top_k}")
    ranked = finder.run()

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)

    for i, scored in enumerate(ranked):
        _inject_variants(scored.scenario)
        fname = outdir / f"worst_case_{i:03d}.yaml"
        header = (
            f"# Adversarial scenario #{i} — divergence score: {scored.score:.4f}\n"
            f"# Discovered by AdversarialFinder (seed={scored.seed}, budget={args.budget})\n"
        )
        with open(fname, "w") as f:
            f.write(header)
            yaml.dump(scored.scenario, f, default_flow_style=False, sort_keys=False)
        print(f"  Saved {fname} (score={scored.score:.4f})")

    print(f"\nDone. {len(ranked)} worst-case scenarios saved to {outdir}/")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
