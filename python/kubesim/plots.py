"""Diagnostic timeseries and distribution plots for KubeSim reports.

Generates PNG charts from batch_run results alongside report.md / report.json.
Uses matplotlib (no interactive backend required).
"""

from __future__ import annotations

from pathlib import Path
from typing import Sequence

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np


# ── Time-axis helpers ─────────────────────────────────────────────

# Heuristic: if max raw time > 1e6, values are nanoseconds (wall_clock mode).
_NS_THRESHOLD = 1e6


def _raw_to_seconds(t: np.ndarray) -> np.ndarray:
    """Convert raw SimTime values to seconds.

    wall_clock mode emits nanoseconds; logical mode emits seconds (1 tick = 1s).
    """
    if len(t) == 0:
        return t
    if t.max() > _NS_THRESHOLD:
        return t / 1e9
    return t


def _time_unit(max_seconds: float) -> tuple[str, float]:
    """Pick human-readable unit and divisor for the x-axis."""
    if max_seconds < 120:
        return "seconds", 1.0
    if max_seconds < 120 * 60:
        return "minutes", 60.0
    return "hours", 3600.0


# ── Timeseries plots ─────────────────────────────────────────────

_TIMESERIES_METRICS = [
    ("pod_count", "Pod Count"),
    ("pending_count", "Pending Pods"),
    ("node_count", "Node Count"),
    ("total_cost_per_hour", "Cost Rate ($/hr)"),
    ("cpu_utilization_p50", "CPU Utilization (p50)"),
    ("memory_utilization_p50", "Memory Utilization (p50)"),
    ("total_vcpu_allocated", "vCPU Allocated"),
    ("total_memory_allocated_gib", "Memory Allocated (GiB)"),
]


# Cumulative metrics computed from timeseries via running integration
_CUMULATIVE_TS_METRICS = [
    ("total_cost_per_hour", "Cumulative Cost ($)", "cumulative_cost"),
    ("total_vcpu_allocated", "Cumulative vCPU-Hours", "cumulative_vcpu_hours"),
    ("total_memory_allocated_gib", "Cumulative GiB-Hours", "cumulative_gib_hours"),
]


def _extract_ts(result: dict) -> list[dict] | None:
    """Get timeseries from a batch_run result dict."""
    ts = result.get("timeseries")
    if ts and len(ts) > 0:
        return ts
    return None


def plot_timeseries(results: list[dict], output_dir: Path) -> list[Path]:
    """Generate per-metric timeseries plots averaged across seeds per variant.

    Returns list of generated file paths.
    """
    # Group timeseries by variant
    by_variant: dict[str, list[list[dict]]] = {}
    for r in results:
        ts = _extract_ts(r)
        if ts is None:
            continue
        v = r.get("variant", "unknown")
        by_variant.setdefault(v, []).append(ts)

    if not by_variant:
        return []

    # Determine time unit from all raw time values
    all_raw = [s["time"] for runs in by_variant.values() for run in runs for s in run]
    max_seconds = _raw_to_seconds(np.array([max(all_raw)], dtype=float))[0] if all_raw else 0.0
    unit_label, unit_div = _time_unit(max_seconds)

    output_dir.mkdir(parents=True, exist_ok=True)
    paths = []

    for metric_key, metric_label in _TIMESERIES_METRICS:
        fig, ax = plt.subplots(figsize=(8, 4))

        for variant, ts_runs in sorted(by_variant.items()):
            all_times = sorted({s["time"] for run in ts_runs for s in run})
            if not all_times:
                continue
            grid = np.array(all_times, dtype=float)
            values = np.zeros((len(ts_runs), len(grid)))

            for i, run in enumerate(ts_runs):
                t = np.array([s["time"] for s in run], dtype=float)
                v = np.array([s.get(metric_key, 0.0) for s in run], dtype=float)
                values[i] = np.interp(grid, t, v)

            display_grid = _raw_to_seconds(grid) / unit_div
            mean = values.mean(axis=0)
            ax.plot(display_grid, mean, label=variant, linewidth=1.5)
            if len(ts_runs) > 1:
                lo, hi = np.percentile(values, [10, 90], axis=0)
                ax.fill_between(display_grid, lo, hi, alpha=0.15)

        ax.set_xlabel(f"Time ({unit_label})")
        ax.set_ylabel(metric_label)
        ax.set_title(metric_label)
        ax.legend()
        ax.grid(True, alpha=0.3)
        fig.tight_layout()

        fname = f"ts_{metric_key}.png"
        path = output_dir / fname
        fig.savefig(path, dpi=120)
        plt.close(fig)
        paths.append(path)

    # Cumulative timeseries (running integral of rate metrics)
    for rate_key, cum_label, cum_fname in _CUMULATIVE_TS_METRICS:
        fig, ax = plt.subplots(figsize=(8, 4))

        for variant, ts_runs in sorted(by_variant.items()):
            all_times = sorted({s["time"] for run in ts_runs for s in run})
            if not all_times:
                continue
            grid = np.array(all_times, dtype=float)
            grid_seconds = _raw_to_seconds(grid)
            cum_values = np.zeros((len(ts_runs), len(grid)))

            for i, run in enumerate(ts_runs):
                t = np.array([s["time"] for s in run], dtype=float)
                v = np.array([s.get(rate_key, 0.0) for s in run], dtype=float)
                interp_v = np.interp(grid, t, v)
                # Trapezoidal cumulative integral; dt in hours for $/hr and rate metrics
                dt = np.diff(grid_seconds)
                avg_rate = (interp_v[:-1] + interp_v[1:]) / 2.0
                increments = avg_rate * dt / 3600.0
                cum_values[i, 1:] = np.cumsum(increments)

            display_grid = grid_seconds / unit_div
            mean = cum_values.mean(axis=0)
            ax.plot(display_grid, mean, label=variant, linewidth=1.5)
            if len(ts_runs) > 1:
                lo, hi = np.percentile(cum_values, [10, 90], axis=0)
                ax.fill_between(display_grid, lo, hi, alpha=0.15)

        ax.set_xlabel(f"Time ({unit_label})")
        ax.set_ylabel(cum_label)
        ax.set_title(cum_label)
        ax.legend()
        ax.grid(True, alpha=0.3)
        fig.tight_layout()

        path = output_dir / f"ts_{cum_fname}.png"
        fig.savefig(path, dpi=120)
        plt.close(fig)
        paths.append(path)

    return paths


# ── Distribution plots ───────────────────────────────────────────

_DISTRIBUTION_METRICS = [
    ("cumulative_cost", "Cumulative Cost ($)"),
    ("cumulative_vcpu_hours", "Cumulative vCPU-Hours"),
    ("cumulative_memory_gib_hours", "Cumulative GiB-Hours"),
    ("node_count", "Final Node Count"),
    ("peak_node_count", "Peak Node Count"),
    ("time_weighted_node_count", "Time-Weighted Node Count"),
    ("cumulative_pending_pod_seconds", "Pending Pod-Seconds"),
    ("disruption_count", "Disruption Count"),
]


def plot_distributions(results: list[dict], output_dir: Path) -> list[Path]:
    """Generate box plots comparing variants across seeds.

    Returns list of generated file paths.
    """
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        v = r.get("variant", "unknown")
        by_variant.setdefault(v, []).append(r)

    if not by_variant:
        return []

    variants = sorted(by_variant.keys())
    output_dir.mkdir(parents=True, exist_ok=True)
    paths = []

    for metric_key, metric_label in _DISTRIBUTION_METRICS:
        data = []
        labels = []
        for v in variants:
            vals = [r.get(metric_key, 0.0) for r in by_variant[v]]
            if any(val != 0.0 for val in vals):
                data.append(vals)
                labels.append(v)

        if not data:
            continue

        fig, ax = plt.subplots(figsize=(6, 4))
        bp = ax.boxplot(data, labels=labels, patch_artist=True)
        colors = plt.cm.Set2(np.linspace(0, 1, len(data)))
        for patch, color in zip(bp["boxes"], colors):
            patch.set_facecolor(color)
            patch.set_alpha(0.7)

        ax.set_ylabel(metric_label)
        ax.set_title(f"{metric_label} by Variant")
        ax.grid(True, axis="y", alpha=0.3)
        fig.tight_layout()

        fname = f"dist_{metric_key}.png"
        path = output_dir / fname
        fig.savefig(path, dpi=120)
        plt.close(fig)
        paths.append(path)

    return paths


# ── Combined entry point ─────────────────────────────────────────

def generate_plots(results: list[dict], output_dir: str | Path) -> list[Path]:
    """Generate all diagnostic plots. Returns list of generated file paths."""
    out = Path(output_dir)
    paths = []
    paths.extend(plot_timeseries(results, out))
    paths.extend(plot_distributions(results, out))
    return paths
