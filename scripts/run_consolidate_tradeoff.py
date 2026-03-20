#!/usr/bin/env python3
"""Run ConsolidateWhen tradeoff analysis: sweep policies × thresholds, plot results.

Usage:
    python scripts/run_consolidate_tradeoff.py \
        --scenario scenarios/benchmark-control.yaml \
        --output results/consolidate-when/benchmark-tradeoff/
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from kubesim.analysis.consolidate_tradeoff import (
    DEFAULT_THRESHOLDS,
    aggregate_metrics,
    build_scenario,
    generate_all_plots,
    generate_markdown_report,
    run_sweep,
)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="ConsolidateWhen tradeoff analysis")
    parser.add_argument("--scenario", required=True, help="Base scenario YAML path")
    parser.add_argument("--output", required=True, help="Output directory for results and plots")
    parser.add_argument("--runs", type=int, default=100, help="Runs per variant (default: 100)")
    parser.add_argument(
        "--thresholds", type=float, nargs="+", default=DEFAULT_THRESHOLDS,
        help="Decision ratio thresholds to sweep",
    )
    args = parser.parse_args(argv)

    scenario_path = Path(args.scenario)
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Loading base scenario: {scenario_path}")
    scenario = build_scenario(scenario_path, args.thresholds, args.runs)

    study = scenario.get("study", scenario)
    study_name = study.get("name", scenario_path.stem)
    n_variants = len(study["variants"])
    seeds = list(range(args.runs))

    print(f"Running {n_variants} variants × {args.runs} seeds...")
    results = run_sweep(scenario, seeds)
    print(f"  Collected {len(results)} results")

    agg = aggregate_metrics(results)

    # Write raw results JSON
    (output_dir / "results.json").write_text(
        json.dumps({k: v for k, v in agg.items()}, indent=2)
    )

    # Generate plots
    print("Generating plots...")
    plots = generate_all_plots(results, agg, output_dir)
    for p in plots:
        print(f"  {p.name}")

    # Generate markdown report
    md = generate_markdown_report(agg, study_name, output_dir)
    (output_dir / "report.md").write_text(md)
    print(f"Report written to {output_dir / 'report.md'}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
