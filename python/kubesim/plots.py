"""Diagnostic timeseries and distribution plots for KubeSim reports.

Generates PNG charts from batch_run results with timeseries data.
Requires matplotlib: pip install matplotlib
"""

from __future__ import annotations

from pathlib import Path
from typing import Sequence

import numpy as np


def _import_mpl():
    """Import matplotlib with Agg backend for headless rendering."""
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    return plt


# ── Timeseries plots ────────────────────────────────────────────


def _time_seconds(snapshots: list[dict]) -> np.ndarray:
    """Extract time in seconds from snapshot dicts."""
    return np.array([s["time"] / 1e9 for s in snapshots])


def plot_timeseries(
    results: list[dict],
    output_dir: str | Path,
    metrics: Sequence[str] | None = None,
) -> list[Path]:
    """Generate per-metric timeseries plots averaged across seeds per variant.

    Args:
        results: batch_run output (list of dicts with 'timeseries' key).
        output_dir: Directory to write PNG files.
        metrics: Which snapshot fields to plot. Defaults to core set.

    Returns:
        List of paths to generated PNG files.
    """
    plt = _import_mpl()
    out = Path(output_dir)
    out.mkdir(parents=True, exist_ok=True)

    if metrics is None:
        metrics = [
            "total_cost_per_hour", "node_count", "pod_count",
            "pending_count", "cpu_p50", "mem_p50", "availability",
        ]

    # Group timeseries by variant
    by_variant: dict[str, list[list[dict]]] = {}
    for r in results:
        ts = r.get("timeseries", [])
        if not ts:
            continue
        v = r["variant"]
        by_variant.setdefault(v, []).append(ts)

    if not by_variant:
        return []

    paths = []
    for metric in metrics:
        fig, ax = plt.subplots(figsize=(8, 4))
        for variant, runs in sorted(by_variant.items()):
            # Collect all unique times, then interpolate each run onto common grid
            all_times = sorted({s["time"] for run in runs for s in run})
            if not all_times:
                continue
            t_grid = np.array([t / 1e9 for t in all_times])
            values = np.full((len(runs), len(t_grid)), np.nan)
            for i, run in enumerate(runs):
                t_run = np.array([s["time"] / 1e9 for s in run])
                v_run = np.array([s.get(metric, np.nan) for s in run])
                values[i] = np.interp(t_grid, t_run, v_run)
            mean = np.nanmean(values, axis=0)
            ax.plot(t_grid, mean, label=variant)
            if len(runs) > 1:
                std = np.nanstd(values, axis=0)
                ax.fill_between(t_grid, mean - std, mean + std, alpha=0.2)
        ax.set_xlabel("Time (s)")
        ax.set_ylabel(metric)
        ax.set_title(metric.replace("_", " ").title())
        ax.legend()
        fig.tight_layout()
        p = out / f"ts_{metric}.png"
        fig.savefig(p, dpi=100)
        plt.close(fig)
        paths.append(p)

    return paths


# ── Distribution plots ──────────────────────────────────────────


def plot_distributions(
    results: list[dict],
    output_dir: str | Path,
    metrics: Sequence[str] | None = None,
) -> list[Path]:
    """Generate box plots of final metrics across seeds per variant.

    Args:
        results: batch_run output (list of dicts).
        output_dir: Directory to write PNG files.
        metrics: Which result fields to plot. Defaults to core set.

    Returns:
        List of paths to generated PNG files.
    """
    plt = _import_mpl()
    out = Path(output_dir)
    out.mkdir(parents=True, exist_ok=True)

    if metrics is None:
        metrics = [
            "cumulative_cost", "peak_node_count", "peak_cost_rate",
            "time_to_stable", "disruption_count",
            "cumulative_pending_pod_seconds",
        ]

    # Group by variant
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r["variant"], []).append(r)

    if not by_variant:
        return []

    variants = sorted(by_variant.keys())
    paths = []
    for metric in metrics:
        data = []
        labels = []
        for v in variants:
            vals = [r.get(metric) for r in by_variant[v] if r.get(metric) is not None]
            if vals:
                data.append(vals)
                labels.append(v)
        if not data:
            continue
        fig, ax = plt.subplots(figsize=(6, 4))
        ax.boxplot(data, labels=labels, showmeans=True)
        ax.set_ylabel(metric)
        ax.set_title(metric.replace("_", " ").title())
        fig.tight_layout()
        p = out / f"dist_{metric}.png"
        fig.savefig(p, dpi=100)
        plt.close(fig)
        paths.append(p)

    return paths


def generate_plots(
    results: list[dict],
    output_dir: str | Path,
) -> list[Path]:
    """Generate all diagnostic plots (timeseries + distributions).

    Returns list of all generated PNG paths.
    """
    paths = []
    paths.extend(plot_timeseries(results, output_dir))
    paths.extend(plot_distributions(results, output_dir))
    return paths
