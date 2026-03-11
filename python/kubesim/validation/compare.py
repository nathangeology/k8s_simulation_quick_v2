"""Cross-tier divergence comparison for KubeSim validation pipeline.

Load SimResult-compatible parquet files from Tier 1 (sim), Tier 2 (KWOK),
and Tier 3 (EKS). Compute per-metric deltas, flag divergences exceeding
configurable thresholds, and generate an HTML report with side-by-side charts.

Usage::

    kubesim compare tier1.parquet tier2.parquet --threshold 0.05 --output report.html
"""

from __future__ import annotations

import html
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Sequence

import polars as pl


# ── Constants ────────────────────────────────────────────────────

COMPARE_METRICS = [
    "total_cost_per_hour",
    "node_count",
    "running_pods",
    "pending_pods",
]

TIER_LABELS = {0: "Tier 1 (Sim)", 1: "Tier 2 (KWOK)", 2: "Tier 3 (EKS)"}


# ── Data structures ─────────────────────────────────────────────

@dataclass
class MetricDelta:
    metric: str
    values: list[float]  # one per tier file
    labels: list[str]
    deltas: list[float]  # pairwise deltas relative to first tier
    pct_deltas: list[float]  # pairwise % deltas
    divergent: list[bool]  # True if |pct_delta| > threshold


@dataclass
class CompareResult:
    tier_files: list[str]
    tier_labels: list[str]
    threshold: float
    metrics: list[MetricDelta]
    has_divergence: bool = False


# ── Core comparison ──────────────────────────────────────────────

def load_tier(path: Path) -> pl.DataFrame:
    """Load a SimResult-compatible parquet file."""
    return pl.read_parquet(path)


def compute_deltas(
    dfs: Sequence[pl.DataFrame],
    labels: Sequence[str],
    metrics: Sequence[str] = COMPARE_METRICS,
    threshold: float = 0.05,
) -> CompareResult:
    """Compute per-metric deltas across tier DataFrames.

    The first DataFrame is treated as the baseline. Deltas are computed
    as (tier_N - baseline) / baseline for each metric's mean value.
    """
    result = CompareResult(
        tier_files=[],
        tier_labels=list(labels),
        threshold=threshold,
        metrics=[],
    )

    for metric in metrics:
        means = []
        for df in dfs:
            if metric in df.columns:
                means.append(df[metric].mean())
            else:
                means.append(0.0)

        baseline = means[0] if means[0] else 1e-9  # avoid div-by-zero
        deltas = [m - means[0] for m in means]
        pct_deltas = [d / abs(baseline) for d in deltas]
        divergent = [abs(p) > threshold for p in pct_deltas]
        # baseline vs itself is never divergent
        divergent[0] = False

        md = MetricDelta(
            metric=metric,
            values=means,
            labels=list(labels),
            deltas=deltas,
            pct_deltas=pct_deltas,
            divergent=divergent,
        )
        result.metrics.append(md)
        if any(divergent):
            result.has_divergence = True

    return result


# ── HTML report generation ───────────────────────────────────────

def _bar_chart_svg(md: MetricDelta, width: int = 400, height: int = 180) -> str:
    """Render a simple inline SVG bar chart for a single metric."""
    n = len(md.values)
    if n == 0:
        return ""
    max_val = max(abs(v) for v in md.values) or 1.0
    bar_w = width // (n * 2)
    margin_left = 10
    chart_h = height - 40

    bars = []
    for i, (val, label, div) in enumerate(zip(md.values, md.labels, md.divergent)):
        bh = int((abs(val) / max_val) * chart_h) if max_val else 0
        x = margin_left + i * bar_w * 2
        y = height - 30 - bh
        color = "#e74c3c" if div else "#3498db"
        esc_label = html.escape(label)
        bars.append(
            f'<rect x="{x}" y="{y}" width="{bar_w}" height="{bh}" fill="{color}" />'
            f'<text x="{x + bar_w // 2}" y="{height - 10}" text-anchor="middle" '
            f'font-size="11" fill="#333">{esc_label}</text>'
            f'<text x="{x + bar_w // 2}" y="{y - 4}" text-anchor="middle" '
            f'font-size="10" fill="#555">{val:.2f}</text>'
        )

    return (
        f'<svg width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg">'
        f'{"".join(bars)}</svg>'
    )


def generate_html_report(result: CompareResult) -> str:
    """Generate a self-contained HTML report with side-by-side charts."""
    status = "DIVERGENCE DETECTED" if result.has_divergence else "ALL WITHIN THRESHOLD"
    status_color = "#e74c3c" if result.has_divergence else "#27ae60"

    rows = []
    charts = []
    for md in result.metrics:
        vals_html = "".join(
            f"<td>{md.values[i]:.4f}</td>" for i in range(len(md.values))
        )
        delta_cells = []
        for i in range(len(md.pct_deltas)):
            if i == 0:
                delta_cells.append("<td>—</td>")
            else:
                pct = md.pct_deltas[i] * 100
                cls = ' class="divergent"' if md.divergent[i] else ""
                delta_cells.append(f"<td{cls}>{pct:+.2f}%</td>")

        esc_metric = html.escape(md.metric)
        rows.append(
            f"<tr><td><strong>{esc_metric}</strong></td>"
            f"{vals_html}{''.join(delta_cells)}</tr>"
        )
        charts.append(
            f"<div class='chart'><h3>{esc_metric}</h3>"
            f"{_bar_chart_svg(md)}</div>"
        )

    tier_headers = "".join(
        f"<th>{html.escape(l)}</th>" for l in result.tier_labels
    )
    delta_headers = "".join(
        f"<th>Δ vs {html.escape(result.tier_labels[0])}</th>"
        for _ in result.tier_labels
    )

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>KubeSim Cross-Tier Comparison</title>
<style>
  body {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; margin: 2em; color: #222; }}
  h1 {{ border-bottom: 2px solid #ccc; padding-bottom: .3em; }}
  .status {{ font-size: 1.2em; font-weight: bold; color: {status_color}; }}
  table {{ border-collapse: collapse; margin: 1em 0; width: 100%; }}
  th, td {{ border: 1px solid #ddd; padding: 8px 12px; text-align: right; }}
  th {{ background: #f5f5f5; }}
  td:first-child, th:first-child {{ text-align: left; }}
  .divergent {{ background: #fdecea; color: #c0392b; font-weight: bold; }}
  .charts {{ display: flex; flex-wrap: wrap; gap: 1.5em; margin-top: 1em; }}
  .chart {{ background: #fafafa; border: 1px solid #eee; border-radius: 6px; padding: 1em; }}
  .chart h3 {{ margin: 0 0 .5em 0; font-size: .95em; }}
  .meta {{ color: #888; font-size: .9em; }}
</style>
</head>
<body>
<h1>KubeSim Cross-Tier Divergence Report</h1>
<p class="status">{status}</p>
<p class="meta">Threshold: {result.threshold * 100:.1f}% &middot;
Tiers: {', '.join(html.escape(l) for l in result.tier_labels)}</p>

<h2>Metric Summary</h2>
<table>
<tr><th>Metric</th>{tier_headers}{delta_headers}</tr>
{''.join(rows)}
</table>

<h2>Side-by-Side Charts</h2>
<div class="charts">
{''.join(charts)}
</div>
</body>
</html>"""


# ── Public API ───────────────────────────────────────────────────

def compare_tiers(
    paths: Sequence[Path | str],
    threshold: float = 0.05,
    metrics: Sequence[str] = COMPARE_METRICS,
    output: Path | str | None = None,
) -> CompareResult:
    """Load parquet files, compute deltas, optionally write HTML report.

    Args:
        paths: Parquet file paths (first is baseline).
        threshold: Divergence threshold as fraction (default 0.05 = 5%).
        metrics: Metric columns to compare.
        output: If provided, write HTML report to this path.

    Returns:
        CompareResult with per-metric deltas and divergence flags.
    """
    paths = [Path(p) for p in paths]
    dfs = [load_tier(p) for p in paths]
    labels = [TIER_LABELS.get(i, f"Tier {i + 1}") for i in range(len(paths))]

    result = compute_deltas(dfs, labels, metrics=metrics, threshold=threshold)
    result.tier_files = [str(p) for p in paths]

    if output:
        output = Path(output)
        output.parent.mkdir(parents=True, exist_ok=True)
        report = generate_html_report(result)
        output.write_text(report)

    return result
