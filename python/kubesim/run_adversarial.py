"""Run AdversarialFinder to discover worst-case MostAllocated vs LeastAllocated divergence.

Saves top-k scenarios as YAML in scenarios/adversarial/.
"""

from __future__ import annotations

import sys
from pathlib import Path

import yaml

from kubesim.adversarial import AdversarialFinder, ScenarioSpace


VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]


def _divergence_metric(results: list[dict]) -> float:
    """Absolute cost divergence between MostAllocated and LeastAllocated variants."""
    by_variant: dict[str, list[float]] = {}
    for r in results:
        v = r.get("variant", "")
        by_variant.setdefault(v, []).append(float(r.get("total_cost_per_hour", 0)))
    most = by_variant.get("most_allocated", [0])
    least = by_variant.get("least_allocated", [0])
    return abs(sum(most) / len(most) - sum(least) / len(least))


def _inject_variants(finder: AdversarialFinder) -> None:
    """Monkey-patch _evaluate to inject both scheduler variants before each evaluation."""
    original = finder._evaluate

    def _patched(scenario, rng):
        study = scenario.get("study", scenario)
        study["variants"] = list(VARIANTS)
        return original(scenario, rng)

    finder._evaluate = _patched


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Adversarial scenario finder: MostAllocated vs LeastAllocated")
    parser.add_argument("--budget", type=int, default=1000)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--seeds", type=int, nargs="+", default=[42, 123, 7])
    parser.add_argument("--outdir", type=str, default=None)
    args = parser.parse_args(argv or sys.argv[2:])

    outdir = Path(args.outdir) if args.outdir else Path(__file__).resolve().parents[2] / "scenarios" / "adversarial"
    outdir.mkdir(parents=True, exist_ok=True)

    space = ScenarioSpace(
        nodes=(10, 1000),
        workload_types=["web_app", "ml_training", "batch_job", "saas_microservice"],
        min_workloads=2,
        max_workloads=8,
        traffic=None,
        min_pools=1,
        max_pools=3,
    )

    finder = AdversarialFinder(
        objective="maximize",
        metric=_divergence_metric,
        space=space,
        budget=args.budget,
        seeds=args.seeds,
        top_k=args.top_k,
        seed=args.seed,
    )
    _inject_variants(finder)

    print(f"Running adversarial search (budget={args.budget}, seeds={args.seeds})...")
    ranked = finder.run()
    print(f"Found {len(ranked)} scenarios. Saving to {outdir}/")

    for i, scored in enumerate(ranked):
        scenario = scored.scenario
        study = scenario.get("study", scenario)
        study["variants"] = list(VARIANTS)
        study["metrics"] = {"compare": ["total_cost", "node_count", "pending_pods"]}
        fname = outdir / f"worst_case_{i:03d}.yaml"
        with open(fname, "w") as f:
            yaml.dump(scenario, f, default_flow_style=False, sort_keys=False)
        print(f"  [{i}] score={scored.score:.4f} -> {fname.name}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
