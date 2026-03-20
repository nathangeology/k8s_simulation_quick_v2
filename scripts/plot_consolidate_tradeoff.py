#!/usr/bin/env python3
"""Plot ConsolidateWhen tradeoff analysis from benchmark-tradeoff results.

Generates 5 plots:
  a. Cost savings vs threshold
  b. Disruption vs threshold
  c. Cost-disruption Pareto frontier
  d. Node count over time per variant
  e. Efficiency frontier (cost_savings_per_disruption vs threshold)

Usage:
  python scripts/plot_consolidate_tradeoff.py [--results-dir results/consolidate-when/benchmark-tradeoff]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np


def _parse_threshold(name: str) -> float | None:
    """Extract numeric threshold from variant name like 'cost-justified-1.50'."""
    m = re.search(r"cost-justified-(\d+\.\d+)", name)
    return float(m.group(1)) if m else None


def _load_results(results_dir: Path) -> tuple[dict, list[dict] | None]:
    """Load report.json and optionally raw results."""
    report_path = results_dir / "report.json"
    if not report_path.exists():
        print(f"Error: {report_path} not found", file=sys.stderr)
        sys.exit(1)
    with open(report_path) as f:
        report = json.load(f)

    raw_path = results_dir / "raw_results.json"
    raw = None
    if raw_path.exists():
        with open(raw_path) as f:
            raw = json.load(f)
    return report, raw


def _get_metric_mean(report: dict, variant: str, metric: str) -> float:
    """Get mean of a metric for a variant from report structure."""
    for section in ("per_variant", "per_variant_diagnostic"):
        if section in report and variant in report[section]:
            if metric in report[section][variant]:
                return report[section][variant][metric]["mean"]
    return 0.0


def plot_cost_savings_vs_threshold(report: dict, output_dir: Path) -> Path:
    """Plot a: Cost savings relative to WhenEmpty baseline vs threshold."""
    baseline_cost = _get_metric_mean(report, "when-empty", "cumulative_cost")
    if baseline_cost == 0:
        baseline_cost = _get_metric_mean(report, "when-empty", "total_cost_per_hour")

    metric = "cumulative_cost" if baseline_cost == _get_metric_mean(report, "when-empty", "cumulative_cost") else "total_cost_per_hour"

    thresholds, savings = [], []
    for v in report["variants"]:
        t = _parse_threshold(v)
        if t is None:
            continue
        cost = _get_metric_mean(report, v, metric)
        saving_pct = (baseline_cost - cost) / baseline_cost * 100 if baseline_cost else 0
        thresholds.append(t)
        savings.append(saving_pct)

    # Also add WhenEmptyOrUnderutilized as a reference
    underutil_cost = _get_metric_mean(report, "when-underutilized", metric)
    underutil_saving = (baseline_cost - underutil_cost) / baseline_cost * 100 if baseline_cost else 0

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, savings, "o-", color="steelblue", linewidth=2, markersize=6, label="WhenCostJustifiesDisruption")
    ax.axhline(underutil_saving, color="orange", linestyle="--", linewidth=1.5, label="WhenEmptyOrUnderutilized")
    ax.axhline(0, color="gray", linestyle=":", linewidth=1, label="WhenEmpty (baseline)")
    ax.set_xlabel("Decision Ratio Threshold")
    ax.set_ylabel("Cost Savings vs WhenEmpty (%)")
    ax.set_title("Cost Savings vs Threshold")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()

    path = output_dir / "cost_savings_vs_threshold.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_disruption_vs_threshold(report: dict, output_dir: Path) -> Path:
    """Plot b: Disruption (pods_evicted or disruption_count) vs threshold."""
    metric = "disruption_count"

    thresholds, disruptions = [], []
    for v in report["variants"]:
        t = _parse_threshold(v)
        if t is None:
            continue
        thresholds.append(t)
        disruptions.append(_get_metric_mean(report, v, metric))

    underutil_disruption = _get_metric_mean(report, "when-underutilized", metric)
    empty_disruption = _get_metric_mean(report, "when-empty", metric)

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, disruptions, "s-", color="crimson", linewidth=2, markersize=6, label="WhenCostJustifiesDisruption")
    ax.axhline(underutil_disruption, color="orange", linestyle="--", linewidth=1.5, label="WhenEmptyOrUnderutilized")
    ax.axhline(empty_disruption, color="gray", linestyle=":", linewidth=1, label="WhenEmpty")
    ax.set_xlabel("Decision Ratio Threshold")
    ax.set_ylabel("Disruption Count (mean)")
    ax.set_title("Disruption vs Threshold")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()

    path = output_dir / "disruption_vs_threshold.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_pareto_frontier(report: dict, output_dir: Path) -> Path:
    """Plot c: Cost-disruption Pareto frontier."""
    cost_metric = "cumulative_cost"
    disruption_metric = "disruption_count"

    baseline_cost = _get_metric_mean(report, "when-empty", cost_metric)

    points = []  # (disruption, cost_savings_pct, label)
    for v in report["variants"]:
        cost = _get_metric_mean(report, v, cost_metric)
        disruption = _get_metric_mean(report, v, disruption_metric)
        saving = (baseline_cost - cost) / baseline_cost * 100 if baseline_cost else 0
        t = _parse_threshold(v)
        label = f"t={t}" if t is not None else v
        points.append((disruption, saving, label, v))

    fig, ax = plt.subplots(figsize=(8, 5))

    # Separate threshold variants from policy variants
    for d, s, label, v in points:
        if "cost-justified" in v:
            ax.scatter(d, s, color="steelblue", s=60, zorder=3)
            ax.annotate(label, (d, s), textcoords="offset points", xytext=(5, 5), fontsize=8)
        elif v == "when-empty":
            ax.scatter(d, s, color="gray", s=80, marker="D", zorder=3)
            ax.annotate("WhenEmpty", (d, s), textcoords="offset points", xytext=(5, 5), fontsize=8)
        elif v == "when-underutilized":
            ax.scatter(d, s, color="orange", s=80, marker="D", zorder=3)
            ax.annotate("WhenUnderutilized", (d, s), textcoords="offset points", xytext=(5, 5), fontsize=8)

    # Connect threshold points with a line
    threshold_pts = [(d, s) for d, s, _, v in points if "cost-justified" in v]
    if threshold_pts:
        threshold_pts.sort()
        ax.plot([p[0] for p in threshold_pts], [p[1] for p in threshold_pts],
                "--", color="steelblue", alpha=0.5, linewidth=1)

    ax.set_xlabel("Disruption Count (mean)")
    ax.set_ylabel("Cost Savings vs WhenEmpty (%)")
    ax.set_title("Cost-Disruption Pareto Frontier")
    ax.grid(True, alpha=0.3)
    fig.tight_layout()

    path = output_dir / "pareto_frontier.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_node_count_over_time(raw_results: list[dict] | None, output_dir: Path) -> Path | None:
    """Plot d: Node count over time per variant (from timeseries data)."""
    if not raw_results:
        return None

    by_variant: dict[str, list[list[dict]]] = {}
    for r in raw_results:
        ts = r.get("timeseries")
        if not ts:
            continue
        v = r.get("variant", "unknown")
        by_variant.setdefault(v, []).append(ts)

    if not by_variant:
        return None

    fig, ax = plt.subplots(figsize=(10, 5))
    cmap = plt.cm.tab10

    for i, (variant, ts_runs) in enumerate(sorted(by_variant.items())):
        all_times = sorted({s["time"] for run in ts_runs for s in run})
        if not all_times:
            continue
        grid = np.array(all_times, dtype=float)
        values = np.zeros((len(ts_runs), len(grid)))
        for j, run in enumerate(ts_runs):
            t = np.array([s["time"] for s in run], dtype=float)
            v = np.array([s.get("node_count", 0) for s in run], dtype=float)
            values[j] = np.interp(grid, t, v)

        # Convert to minutes
        grid_sec = grid / 1e9 if grid.max() > 1e6 else grid
        grid_min = grid_sec / 60.0
        mean = values.mean(axis=0)
        ax.plot(grid_min, mean, label=variant, linewidth=1.2, color=cmap(i / max(len(by_variant) - 1, 1)))

    ax.set_xlabel("Time (minutes)")
    ax.set_ylabel("Node Count")
    ax.set_title("Node Count Over Time by Variant")
    ax.legend(fontsize=7, ncol=2)
    ax.grid(True, alpha=0.3)
    fig.tight_layout()

    path = output_dir / "node_count_over_time.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_efficiency_frontier(report: dict, output_dir: Path) -> Path:
    """Plot e: Cost savings per disruption vs threshold (sweet spot finder)."""
    cost_metric = "cumulative_cost"
    disruption_metric = "disruption_count"
    baseline_cost = _get_metric_mean(report, "when-empty", cost_metric)

    thresholds, efficiencies = [], []
    for v in report["variants"]:
        t = _parse_threshold(v)
        if t is None:
            continue
        cost = _get_metric_mean(report, v, cost_metric)
        disruption = _get_metric_mean(report, v, disruption_metric)
        saving = (baseline_cost - cost) / baseline_cost * 100 if baseline_cost else 0
        efficiency = saving / disruption if disruption > 0 else saving
        thresholds.append(t)
        efficiencies.append(efficiency)

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, efficiencies, "^-", color="seagreen", linewidth=2, markersize=7)

    # Mark the peak
    if efficiencies:
        best_idx = int(np.argmax(efficiencies))
        ax.annotate(f"Sweet spot: t={thresholds[best_idx]}",
                     (thresholds[best_idx], efficiencies[best_idx]),
                     textcoords="offset points", xytext=(10, 10), fontsize=9,
                     arrowprops=dict(arrowstyle="->", color="seagreen"))

    ax.set_xlabel("Decision Ratio Threshold")
    ax.set_ylabel("Cost Savings (%) per Disruption Event")
    ax.set_title("Efficiency Frontier: Savings per Disruption")
    ax.grid(True, alpha=0.3)
    fig.tight_layout()

    path = output_dir / "efficiency_frontier.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Plot ConsolidateWhen tradeoff analysis")
    parser.add_argument("--results-dir", default="results/consolidate-when/benchmark-tradeoff",
                        help="Directory containing report.json (and optionally raw_results.json)")
    parser.add_argument("--output-dir", default=None,
                        help="Output directory for plots (defaults to results-dir)")
    args = parser.parse_args(argv)

    results_dir = Path(args.results_dir)
    output_dir = Path(args.output_dir) if args.output_dir else results_dir

    report, raw = _load_results(results_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    paths = []
    paths.append(plot_cost_savings_vs_threshold(report, output_dir))
    paths.append(plot_disruption_vs_threshold(report, output_dir))
    paths.append(plot_pareto_frontier(report, output_dir))
    node_path = plot_node_count_over_time(raw, output_dir)
    if node_path:
        paths.append(node_path)
    paths.append(plot_efficiency_frontier(report, output_dir))

    print(f"Generated {len(paths)} plots in {output_dir}/")
    for p in paths:
        print(f"  {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
