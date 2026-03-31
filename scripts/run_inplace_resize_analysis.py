#!/usr/bin/env python3
"""Run InPlacePodVerticalScaling scenario analysis across consolidation strategies.

Usage:
    python scripts/run_inplace_resize_analysis.py [--runs 20] [--output results/inplace-resize]
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import yaml

from kubesim._native import batch_run


SCENARIOS = [
    "scenarios/inplace-resize/jvm-startup-spike.yaml",
    "scenarios/inplace-resize/vpa-gradual-scaleup.yaml",
    "scenarios/inplace-resize/vpa-random-jitter.yaml",
]

KEY_METRICS = [
    "disruption_count",
    "cumulative_cost",
    "time_weighted_node_count",
    "peak_node_count",
    "pods_evicted",
]


def load_and_run(scenario_path: Path, runs: int) -> tuple[str, dict, list[dict]]:
    """Load scenario, run batch, return (name, study, results)."""
    with open(scenario_path) as f:
        scenario = yaml.safe_load(f)
    study = scenario.get("study", scenario)
    study["runs"] = runs
    config = yaml.dump(scenario, default_flow_style=False)
    seeds = list(range(runs))
    raw = batch_run(config, seeds)
    results = [dict(r) if not isinstance(r, dict) else r for r in raw]
    return study["name"], study, results


def aggregate(results: list[dict]) -> dict[str, dict]:
    """Group by variant, compute mean/std for each metric."""
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        v = r.get("variant", "baseline")
        by_variant.setdefault(v, []).append(r)

    agg = {}
    for variant, runs in by_variant.items():
        stats = {}
        for m in KEY_METRICS:
            vals = [r.get(m, 0) for r in runs]
            stats[m] = {"mean": float(np.mean(vals)), "std": float(np.std(vals)), "n": len(vals)}
        agg[variant] = stats
    return agg


def plot_comparison(all_agg: dict[str, dict[str, dict]], output_dir: Path) -> list[Path]:
    """Generate comparison bar charts across scenarios."""
    plots = []

    for metric in KEY_METRICS:
        fig, axes = plt.subplots(1, len(all_agg), figsize=(5 * len(all_agg), 5), sharey=True)
        if len(all_agg) == 1:
            axes = [axes]

        for ax, (scenario_name, agg) in zip(axes, all_agg.items()):
            variants = sorted(agg.keys())
            means = [agg[v][metric]["mean"] for v in variants]
            stds = [agg[v][metric]["std"] for v in variants]
            x = np.arange(len(variants))
            ax.bar(x, means, yerr=stds, capsize=3, alpha=0.8)
            ax.set_xticks(x)
            ax.set_xticklabels(variants, rotation=45, ha="right", fontsize=7)
            ax.set_title(scenario_name, fontsize=9)
            ax.set_ylabel(metric)

        fig.suptitle(metric.replace("_", " ").title(), fontsize=12)
        fig.tight_layout()
        p = output_dir / f"compare-{metric}.png"
        fig.savefig(p, dpi=150)
        plt.close(fig)
        plots.append(p)

    return plots


def plot_cost_vs_disruption(all_agg: dict[str, dict[str, dict]], output_dir: Path) -> Path:
    """Scatter plot: cumulative_cost vs disruption_count per variant, one series per scenario."""
    fig, ax = plt.subplots(figsize=(8, 6))
    markers = ["o", "s", "^", "D", "v"]
    for i, (scenario_name, agg) in enumerate(all_agg.items()):
        variants = sorted(agg.keys())
        costs = [agg[v]["cumulative_cost"]["mean"] for v in variants]
        disruptions = [agg[v]["disruption_count"]["mean"] for v in variants]
        ax.scatter(costs, disruptions, label=scenario_name, marker=markers[i % len(markers)], s=60)
        for v, c, d in zip(variants, costs, disruptions):
            ax.annotate(v, (c, d), fontsize=5, alpha=0.7, xytext=(3, 3), textcoords="offset points")

    ax.set_xlabel("Cumulative Cost ($)")
    ax.set_ylabel("Disruption Count")
    ax.set_title("Cost vs Disruption: InPlace Resize Scenarios")
    ax.legend(fontsize=8)
    fig.tight_layout()
    p = output_dir / "cost-vs-disruption.png"
    fig.savefig(p, dpi=150)
    plt.close(fig)
    return p


def generate_report(all_agg: dict[str, dict[str, dict]], output_dir: Path) -> str:
    """Generate markdown report."""
    lines = ["# InPlacePodVerticalScaling + Consolidation Analysis\n"]
    lines.append("Key question from karpenter#829: *How do we keep consolidation from causing")
    lines.append("too much disruption and cancelling out the potential savings?*\n")

    for scenario_name, agg in all_agg.items():
        lines.append(f"## {scenario_name}\n")
        variants = sorted(agg.keys())
        # Table header
        header = "| Variant | " + " | ".join(m.replace("_", " ") for m in KEY_METRICS) + " |"
        sep = "|" + "|".join(["---"] * (len(KEY_METRICS) + 1)) + "|"
        lines.append(header)
        lines.append(sep)
        for v in variants:
            row = f"| {v} | "
            row += " | ".join(f"{agg[v][m]['mean']:.2f} ± {agg[v][m]['std']:.2f}" for m in KEY_METRICS)
            row += " |"
            lines.append(row)
        lines.append("")

    lines.append("## Plots\n")
    for m in KEY_METRICS:
        lines.append(f"![{m}](compare-{m}.png)\n")
    lines.append("![Cost vs Disruption](cost-vs-disruption.png)\n")

    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="InPlace resize consolidation analysis")
    parser.add_argument("--runs", type=int, default=20, help="Runs per variant (default: 20)")
    parser.add_argument("--output", default="results/inplace-resize", help="Output directory")
    args = parser.parse_args(argv)

    base = Path(os.path.dirname(__file__)).parent
    output_dir = base / args.output
    output_dir.mkdir(parents=True, exist_ok=True)

    all_agg: dict[str, dict[str, dict]] = {}

    for scenario_rel in SCENARIOS:
        scenario_path = base / scenario_rel
        if not scenario_path.exists():
            print(f"SKIP: {scenario_path} not found")
            continue
        print(f"Running {scenario_path.name}...")
        name, study, results = load_and_run(scenario_path, args.runs)
        agg = aggregate(results)
        all_agg[name] = agg

        # Write per-scenario results
        per_dir = output_dir / name
        per_dir.mkdir(parents=True, exist_ok=True)
        (per_dir / "results.json").write_text(json.dumps(agg, indent=2))
        print(f"  {name}: {len(results)} results, {len(agg)} variants")

    if not all_agg:
        print("ERROR: No scenarios ran successfully")
        return 1

    # Generate comparison plots
    print("Generating plots...")
    plots = plot_comparison(all_agg, output_dir)
    for p in plots:
        print(f"  {p.name}")
    scatter = plot_cost_vs_disruption(all_agg, output_dir)
    print(f"  {scatter.name}")

    # Generate report
    report = generate_report(all_agg, output_dir)
    (output_dir / "report.md").write_text(report)
    print(f"Report: {output_dir / 'report.md'}")

    # Dump combined results
    (output_dir / "all-results.json").write_text(json.dumps(all_agg, indent=2))

    return 0


if __name__ == "__main__":
    sys.exit(main())
