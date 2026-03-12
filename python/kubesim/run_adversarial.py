"""Run AdversarialFinder to discover worst-case scenario divergence.

Supports multi-objective search, chaos mode, and arbitrary variant comparison.

Usage::

    python -m kubesim.run_adversarial [--budget 1000] [--top-k 10] [--chaos] [--variants karpenter]

"""

from __future__ import annotations

import argparse
from pathlib import Path

import yaml

from kubesim.adversarial import (
    AdversarialFinder, ScenarioSpace,
    MOST_VS_LEAST, KARPENTER_CONSOLIDATION, DELETION_COST_PAIRS,
)
from kubesim.objectives import OBJECTIVES

VARIANT_PAIRS = {
    "scoring": MOST_VS_LEAST,
    "karpenter": KARPENTER_CONSOLIDATION,
    "deletion_cost": DELETION_COST_PAIRS,
}


def _divergence_metric(results: list[dict]) -> float:
    """Absolute difference in mean total_cost_per_hour between the two variants."""
    variants = set(r.get("variant", "") for r in results)
    if len(variants) < 2:
        return 0.0
    by_variant: dict[str, list[float]] = {}
    for r in results:
        by_variant.setdefault(r.get("variant", ""), []).append(r.get("total_cost_per_hour", 0))
    means = [sum(v) / len(v) for v in by_variant.values() if v]
    return abs(means[0] - means[1]) if len(means) >= 2 else 0.0


def _inject_variants(scenario: dict, pair) -> dict:
    """Ensure scenario has the specified variant pair."""
    study = scenario.get("study", scenario)
    study["variants"] = [pair.config_a, pair.config_b]
    return scenario


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Adversarial scenario finder")
    parser.add_argument("--budget", type=int, default=1000)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--outdir", type=str, default="scenarios/adversarial")
    parser.add_argument("--chaos", action="store_true", help="Enable chaos mode")
    parser.add_argument("--variants", choices=list(VARIANT_PAIRS), default="scoring",
                        help="Variant pair to compare")
    parser.add_argument("--objectives", nargs="*", choices=list(OBJECTIVES), default=[],
                        help="Additional objectives to track")
    parser.add_argument("--track-features", action="store_true",
                        help="Track feature importance")
    args = parser.parse_args(argv)

    pair = VARIANT_PAIRS[args.variants]
    extra_objectives = [OBJECTIVES[name] for name in args.objectives]

    space = ScenarioSpace(
        nodes=(10, 1000),
        workload_types=None,
        min_workloads=2,
        max_workloads=8,
        traffic=None,
        min_pools=1,
        max_pools=3,
    )

    finder = AdversarialFinder(
        objective="maximize",
        metric=_divergence_metric,
        objectives=extra_objectives,
        space=space,
        budget=args.budget,
        seeds=[42, 123, 7],
        top_k=args.top_k,
        seed=args.seed,
        chaos=args.chaos,
        variant_pair=pair,
        track_features=args.track_features,
    )

    label = f"{pair.name_a} vs {pair.name_b}"
    print(f"Running adversarial search: {label}, budget={args.budget}, chaos={args.chaos}")
    ranked = finder.run()

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)

    for i, scored in enumerate(ranked):
        _inject_variants(scored.scenario, pair)
        fname = outdir / f"worst_case_{i:03d}.yaml"
        header = (
            f"# Adversarial scenario #{i} -- divergence score: {scored.score:.4f}\n"
            f"# {label} (seed={scored.seed}, budget={args.budget}, chaos={args.chaos})\n"
        )
        if scored.objective_scores:
            header += f"# Objective scores: {scored.objective_scores}\n"
        with open(fname, "w") as f:
            f.write(header)
            yaml.dump(scored.scenario, f, default_flow_style=False, sort_keys=False)
        print(f"  Saved {fname} (score={scored.score:.4f})")

    if args.track_features and hasattr(finder, "feature_importance"):
        print("\nFeature importance (avg |score| * feature_value):")
        for feat, imp in sorted(finder.feature_importance.items(), key=lambda x: -x[1]):
            print(f"  {feat:<30} {imp:.4f}")

    print(f"\nDone. {len(ranked)} worst-case scenarios saved to {outdir}/")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
