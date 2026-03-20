"""ConsolidateWhen tradeoff analysis: sweep policies × thresholds, collect metrics, plot.

Reusable functions for running consolidation policy sweeps and generating
cost-vs-disruption tradeoff plots.  Importable for custom analysis or called
via ``scripts/run_consolidate_tradeoff.py``.
"""

from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Sequence

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import yaml


# ── Variant generation ────────────────────────────────────────────

# Baseline policies (no threshold parameter)
BASELINE_POLICIES = ["WhenEmpty", "WhenEmptyOrUnderutilized"]

DEFAULT_THRESHOLDS = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0]


def generate_variants(
    thresholds: Sequence[float] = DEFAULT_THRESHOLDS,
) -> list[dict]:
    """Build variant dicts crossing ConsolidateWhen policies × thresholds."""
    variants = []
    for policy in BASELINE_POLICIES:
        variants.append({
            "name": policy,
            "consolidate_when": {"policy": policy},
        })
    for t in thresholds:
        variants.append({
            "name": f"CostJustified-{t:.2f}",
            "consolidate_when": {
                "policy": "WhenCostJustifiesDisruption",
                "decision_ratio_threshold": t,
            },
        })
    return variants


def build_scenario(
    base_path: str | Path,
    thresholds: Sequence[float] = DEFAULT_THRESHOLDS,
    runs: int = 100,
) -> dict:
    """Load a base scenario YAML and inject consolidation sweep variants."""
    with open(base_path) as f:
        scenario = yaml.safe_load(f)

    study = scenario.get("study", scenario)
    study["runs"] = runs
    study["variants"] = generate_variants(thresholds)
    return scenario


# ── Simulation runner ─────────────────────────────────────────────

def run_sweep(scenario: dict, seeds: list[int]) -> list[dict]:
    """Run batch_run for the scenario and return result dicts."""
    from kubesim._native import batch_run

    config_yaml = yaml.dump(scenario, default_flow_style=False)
    raw = batch_run(config_yaml, seeds)
    return [dict(r) if not isinstance(r, dict) else r for r in raw]


# ── Metrics aggregation ──────────────────────────────────────────

def aggregate_metrics(results: list[dict]) -> dict[str, dict]:
    """Group results by variant, compute mean metrics.

    Returns ``{variant_name: {metric: mean_value, ...}}``.
    """
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r["variant"], []).append(r)

    agg = {}
    for variant, runs in sorted(by_variant.items()):
        n = len(runs)
        agg[variant] = {
            "cumulative_cost": sum(r.get("cumulative_cost", 0) for r in runs) / n,
            "disruption_count": sum(r.get("disruption_count", 0) for r in runs) / n,
            "node_count": sum(r.get("node_count", 0) for r in runs) / n,
            "peak_node_count": sum(r.get("peak_node_count", 0) for r in runs) / n,
            "time_weighted_node_count": sum(r.get("time_weighted_node_count", 0) for r in runs) / n,
            "total_cost_per_hour": sum(r.get("total_cost_per_hour", 0) for r in runs) / n,
            "cumulative_pending_pod_seconds": sum(r.get("cumulative_pending_pod_seconds", 0) for r in runs) / n,
            "runs": n,
        }
    return agg


def _parse_threshold(variant_name: str) -> float | None:
    """Extract threshold from variant name like 'CostJustified-1.50'."""
    if variant_name.startswith("CostJustified-"):
        try:
            return float(variant_name.split("-", 1)[1])
        except ValueError:
            pass
    return None


# ── Plotting ──────────────────────────────────────────────────────

def _threshold_series(agg: dict[str, dict]) -> tuple[list[float], list[str]]:
    """Return sorted (thresholds, variant_names) for CostJustified variants."""
    pairs = []
    for name in agg:
        t = _parse_threshold(name)
        if t is not None:
            pairs.append((t, name))
    pairs.sort()
    return [p[0] for p in pairs], [p[1] for p in pairs]


def plot_cost_savings_vs_threshold(
    agg: dict[str, dict], output_dir: Path,
) -> Path:
    """Plot a: cost savings (% relative to WhenEmpty) vs threshold."""
    thresholds, names = _threshold_series(agg)
    baseline_cost = agg.get("WhenEmpty", {}).get("cumulative_cost", 1.0) or 1.0
    savings = [
        (1.0 - agg[n]["cumulative_cost"] / baseline_cost) * 100 for n in names
    ]

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, savings, "o-", linewidth=2, markersize=6)
    ax.set_xlabel("Decision Ratio Threshold")
    ax.set_ylabel("Cost Savings vs WhenEmpty (%)")
    ax.set_title("Cost Savings vs Threshold")
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    path = output_dir / "cost_savings_vs_threshold.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_disruption_vs_threshold(
    agg: dict[str, dict], output_dir: Path,
) -> Path:
    """Plot b: disruption (mean disruption_count) vs threshold."""
    thresholds, names = _threshold_series(agg)
    disruptions = [agg[n]["disruption_count"] for n in names]

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, disruptions, "s-", color="tab:red", linewidth=2, markersize=6)
    ax.set_xlabel("Decision Ratio Threshold")
    ax.set_ylabel("Mean Disruption Count")
    ax.set_title("Disruption vs Threshold")
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    path = output_dir / "disruption_vs_threshold.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_pareto_frontier(
    agg: dict[str, dict], output_dir: Path,
) -> Path:
    """Plot c: cost-disruption Pareto frontier (all variants labeled)."""
    baseline_cost = agg.get("WhenEmpty", {}).get("cumulative_cost", 1.0) or 1.0

    fig, ax = plt.subplots(figsize=(9, 6))
    for name, m in sorted(agg.items()):
        savings = (1.0 - m["cumulative_cost"] / baseline_cost) * 100
        disruption = m["disruption_count"]
        t = _parse_threshold(name)
        label = f"t={t}" if t is not None else name
        ax.scatter(disruption, savings, s=60, zorder=5)
        ax.annotate(label, (disruption, savings), textcoords="offset points",
                    xytext=(5, 5), fontsize=8)

    ax.set_xlabel("Mean Disruption Count")
    ax.set_ylabel("Cost Savings vs WhenEmpty (%)")
    ax.set_title("Cost-Disruption Pareto Frontier")
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    path = output_dir / "pareto_frontier.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_node_count_over_time(
    results: list[dict], output_dir: Path,
) -> Path:
    """Plot d: node count timeseries, one line per variant (mean across seeds)."""
    from kubesim.plots import _raw_to_seconds, _time_unit

    by_variant: dict[str, list[list[dict]]] = {}
    for r in results:
        ts = r.get("timeseries")
        if not ts:
            continue
        by_variant.setdefault(r["variant"], []).append(ts)

    if not by_variant:
        # No timeseries data — create empty plot
        fig, ax = plt.subplots(figsize=(8, 5))
        ax.set_title("Node Count Over Time (no timeseries data)")
        fig.tight_layout()
        path = output_dir / "node_count_over_time.png"
        fig.savefig(path, dpi=150)
        plt.close(fig)
        return path

    all_raw = [s["time"] for runs in by_variant.values() for run in runs for s in run]
    max_sec = _raw_to_seconds(np.array([max(all_raw)], dtype=float))[0] if all_raw else 0
    unit_label, unit_div = _time_unit(max_sec)

    fig, ax = plt.subplots(figsize=(10, 5))
    for variant, ts_runs in sorted(by_variant.items()):
        all_times = sorted({s["time"] for run in ts_runs for s in run})
        if not all_times:
            continue
        grid = np.array(all_times, dtype=float)
        values = np.zeros((len(ts_runs), len(grid)))
        for i, run in enumerate(ts_runs):
            t = np.array([s["time"] for s in run], dtype=float)
            v = np.array([s.get("node_count", 0) for s in run], dtype=float)
            values[i] = np.interp(grid, t, v)
        display = _raw_to_seconds(grid) / unit_div
        ax.plot(display, values.mean(axis=0), label=variant, linewidth=1.2)

    ax.set_xlabel(f"Time ({unit_label})")
    ax.set_ylabel("Node Count")
    ax.set_title("Node Count Over Time")
    ax.legend(fontsize=7, ncol=2)
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    path = output_dir / "node_count_over_time.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def plot_efficiency_frontier(
    agg: dict[str, dict], output_dir: Path,
) -> Path:
    """Plot e: cost_savings_per_disruption vs threshold — sweet spot finder."""
    baseline_cost = agg.get("WhenEmpty", {}).get("cumulative_cost", 1.0) or 1.0
    thresholds, names = _threshold_series(agg)

    efficiency = []
    for n in names:
        savings = (1.0 - agg[n]["cumulative_cost"] / baseline_cost) * 100
        disruption = agg[n]["disruption_count"]
        efficiency.append(savings / disruption if disruption > 0 else 0.0)

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, efficiency, "D-", color="tab:green", linewidth=2, markersize=6)
    if efficiency:
        best_idx = int(np.argmax(efficiency))
        ax.axvline(thresholds[best_idx], color="tab:orange", linestyle="--", alpha=0.7,
                    label=f"Sweet spot: t={thresholds[best_idx]}")
        ax.legend()
    ax.set_xlabel("Decision Ratio Threshold")
    ax.set_ylabel("Cost Savings (%) per Disruption")
    ax.set_title("Efficiency Frontier — Sweet Spot")
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    path = output_dir / "efficiency_frontier.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def generate_all_plots(
    results: list[dict],
    agg: dict[str, dict],
    output_dir: Path,
) -> list[Path]:
    """Generate all 5 tradeoff plots. Returns list of paths."""
    output_dir.mkdir(parents=True, exist_ok=True)
    return [
        plot_cost_savings_vs_threshold(agg, output_dir),
        plot_disruption_vs_threshold(agg, output_dir),
        plot_pareto_frontier(agg, output_dir),
        plot_node_count_over_time(results, output_dir),
        plot_efficiency_frontier(agg, output_dir),
    ]


# ── Report generation ─────────────────────────────────────────────

def generate_markdown_report(
    agg: dict[str, dict],
    scenario_name: str,
    output_dir: Path,
) -> str:
    """Generate a markdown summary of the tradeoff analysis."""
    baseline_cost = agg.get("WhenEmpty", {}).get("cumulative_cost", 1.0) or 1.0
    lines = [
        f"# ConsolidateWhen Tradeoff Analysis: {scenario_name}",
        "",
        f"Variants: {len(agg)}  ",
        f"Runs per variant: {next(iter(agg.values()))['runs']}",
        "",
        "## Summary Table",
        "",
        "| Variant | Cumulative Cost | Cost Savings (%) | Disruptions | Nodes (TWA) |",
        "|---------|----------------|-------------------|-------------|-------------|",
    ]
    for name, m in sorted(agg.items()):
        savings = (1.0 - m["cumulative_cost"] / baseline_cost) * 100
        lines.append(
            f"| {name} | {m['cumulative_cost']:.2f} | {savings:+.2f}% "
            f"| {m['disruption_count']:.1f} | {m['time_weighted_node_count']:.1f} |"
        )

    # Sweet spot analysis
    thresholds, names = _threshold_series(agg)
    if thresholds:
        efficiencies = []
        for n in names:
            s = (1.0 - agg[n]["cumulative_cost"] / baseline_cost) * 100
            d = agg[n]["disruption_count"]
            efficiencies.append(s / d if d > 0 else 0.0)
        best_idx = int(np.argmax(efficiencies))
        lines.extend([
            "",
            "## Sweet Spot",
            "",
            f"Best cost-savings-per-disruption at threshold **{thresholds[best_idx]}** "
            f"(efficiency: {efficiencies[best_idx]:.2f}% savings per disruption).",
        ])

    lines.extend([
        "",
        "## Plots",
        "",
        "![Cost Savings vs Threshold](cost_savings_vs_threshold.png)",
        "![Disruption vs Threshold](disruption_vs_threshold.png)",
        "![Pareto Frontier](pareto_frontier.png)",
        "![Node Count Over Time](node_count_over_time.png)",
        "![Efficiency Frontier](efficiency_frontier.png)",
    ])

    return "\n".join(lines)
