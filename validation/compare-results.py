#!/usr/bin/env python3
"""compare-results.py — Compare kubesim simulation results with real cluster metrics.

Loads sim results (report.json from kubesim run) and real cluster metrics
(parquet from metrics_collector), produces a fidelity comparison report.

Usage:
    python validation/compare-results.py \\
        results/benchmark-control/benchmark-control/report.json \\
        validation/results/smoke-test/metrics.parquet \\
        --output validation/results/comparison.json
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any


class Fidelity(str, Enum):
    GREEN = "GREEN"
    YELLOW = "YELLOW"
    RED = "RED"


@dataclass
class MetricComparison:
    metric: str
    sim_value: float
    real_value: float
    delta_pct: float
    fidelity: Fidelity


def _icon(f: Fidelity) -> str:
    return {"GREEN": "🟢", "YELLOW": "🟡", "RED": "🔴"}[f.value]


def _rate(delta_pct: float, threshold: float = 20.0) -> Fidelity:
    a = abs(delta_pct)
    if a < threshold:
        return Fidelity.GREEN
    if a < threshold * 2:
        return Fidelity.YELLOW
    return Fidelity.RED


def load_sim_results(path: Path) -> dict[str, Any]:
    """Load kubesim report.json."""
    with open(path) as f:
        return json.load(f)


def load_real_metrics(path: Path) -> list[dict[str, Any]]:
    """Load real cluster metrics from parquet or JSON."""
    if path.suffix == ".parquet":
        try:
            import polars as pl
            return pl.read_parquet(path).to_dicts()
        except ImportError:
            import pyarrow.parquet as pq
            return pq.read_table(str(path)).to_pylist()
    with open(path) as f:
        return json.load(f)


def summarize_real(snapshots: list[dict[str, Any]]) -> dict[str, float]:
    """Compute summary stats from time-series snapshots."""
    if not snapshots:
        return {}
    n = len(snapshots)
    return {
        "node_count": snapshots[-1].get("node_count", 0),
        "peak_node_count": max(s.get("node_count", 0) for s in snapshots),
        "running_pods": snapshots[-1].get("running_pods", 0),
        "pending_pods": snapshots[-1].get("pending_pods", 0),
        "cost_per_hour": snapshots[-1].get("cost_per_hour", 0.0),
        "peak_cost_per_hour": max(s.get("cost_per_hour", 0.0) for s in snapshots),
        "avg_node_count": sum(s.get("node_count", 0) for s in snapshots) / n,
    }


def compare(
    sim: dict[str, Any],
    real_summary: dict[str, float],
    threshold: float = 20.0,
) -> list[MetricComparison]:
    """Compare sim report metrics against real cluster summary."""
    variants = sim.get("per_variant", {})
    variant_name = next(iter(variants), None)
    if not variant_name:
        return []
    sv = variants[variant_name]
    diag = sim.get("per_variant_diagnostic", {}).get(variant_name, {})

    pairs: list[tuple[str, float, float]] = []

    if "peak_node_count" in sv and "peak_node_count" in real_summary:
        pairs.append(("peak_node_count", sv["peak_node_count"].get("mean", 0), real_summary["peak_node_count"]))
    if "node_count" in diag and "node_count" in real_summary:
        pairs.append(("final_node_count", diag["node_count"].get("mean", 0), real_summary["node_count"]))
    if "total_cost_per_hour" in diag and "cost_per_hour" in real_summary:
        pairs.append(("cost_per_hour", diag["total_cost_per_hour"].get("mean", 0), real_summary["cost_per_hour"]))
    if "running_pods" in diag and "running_pods" in real_summary:
        pairs.append(("running_pods", diag["running_pods"].get("mean", 0), real_summary["running_pods"]))
    if "pending_pods" in diag and "pending_pods" in real_summary:
        pairs.append(("pending_pods", diag["pending_pods"].get("mean", 0), real_summary["pending_pods"]))

    results = []
    for metric, sim_val, real_val in pairs:
        base = abs(sim_val) if sim_val != 0 else 1.0
        delta_pct = ((real_val - sim_val) / base) * 100
        results.append(MetricComparison(
            metric=metric, sim_value=sim_val, real_value=real_val,
            delta_pct=delta_pct, fidelity=_rate(delta_pct, threshold),
        ))
    return results


def format_report(comparisons: list[MetricComparison]) -> dict[str, Any]:
    """Format comparison results as JSON-serializable dict."""
    overall = Fidelity.GREEN
    for c in comparisons:
        if c.fidelity == Fidelity.RED:
            overall = Fidelity.RED
        elif c.fidelity == Fidelity.YELLOW and overall != Fidelity.RED:
            overall = Fidelity.YELLOW
    return {
        "overall_fidelity": overall.value,
        "metrics": [
            {
                "metric": c.metric,
                "sim_value": c.sim_value,
                "real_value": c.real_value,
                "delta_pct": round(c.delta_pct, 2),
                "fidelity": c.fidelity.value,
            }
            for c in comparisons
        ],
    }


def format_markdown(comparisons: list[MetricComparison]) -> str:
    """Format comparison as markdown table."""
    report = format_report(comparisons)
    lines = [
        "# Sim vs Real Fidelity Report",
        "",
        f"**Overall: {_icon(Fidelity(report['overall_fidelity']))} {report['overall_fidelity']}**",
        "",
        "| Metric | Sim | Real | Δ% | Rating |",
        "|--------|----:|-----:|---:|--------|",
    ]
    for c in comparisons:
        lines.append(
            f"| {c.metric} | {c.sim_value:.2f} | {c.real_value:.2f} "
            f"| {c.delta_pct:+.1f}% | {_icon(c.fidelity)} {c.fidelity.value} |"
        )
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Compare sim results with real cluster metrics")
    parser.add_argument("sim_results", type=Path, help="Sim report.json")
    parser.add_argument("real_metrics", type=Path, help="Real metrics parquet/json")
    parser.add_argument("--output", "-o", type=Path, help="Output JSON report")
    parser.add_argument("--threshold", "-t", type=float, default=20.0,
                        help="Divergence threshold %% (default: 20)")
    args = parser.parse_args(argv)

    sim = load_sim_results(args.sim_results)
    snapshots = load_real_metrics(args.real_metrics)
    real_summary = summarize_real(snapshots)
    comparisons = compare(sim, real_summary, args.threshold)

    print(format_markdown(comparisons))

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(format_report(comparisons), indent=2))
        print(f"Report written to {args.output}")

    return 1 if any(c.fidelity == Fidelity.RED for c in comparisons) else 0


if __name__ == "__main__":
    sys.exit(main())
