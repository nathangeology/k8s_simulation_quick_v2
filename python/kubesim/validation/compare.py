"""Cross-tier divergence comparison for KubeSim validation pipeline.

Supports two comparison modes:

1. **Percentage-delta mode** (original): scalar % deltas between tier means.
2. **Sigma-scoring mode** (new): z-score of observed value against Tier 1
   distribution, with per-metric tolerance and fidelity ratings.

Additionally provides DTW (Dynamic Time Warping) for time-series comparison
of ``node_count_over_time`` and similar temporal metrics.

Usage::

    # Original mode
    kubesim compare tier1.parquet tier2.parquet --threshold 0.05

    # Sigma scoring mode
    kubesim compare tier1_runs/ tier2.parquet --mode sigma
"""

from __future__ import annotations

import html
import json
import math
from dataclasses import dataclass, field
from enum import Enum
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

# Per-metric sigma tolerance: how many σ before YELLOW / RED.
# Default is 1σ GREEN, 2σ YELLOW, >=2σ RED.
# scheduling_latency is noisier so gets 2σ GREEN threshold.
DEFAULT_TOLERANCES: dict[str, float] = {
    "scheduling_latency": 2.0,
    "scheduling_latency_p50": 2.0,
    "scheduling_latency_p99": 2.0,
}
DEFAULT_SIGMA_THRESHOLD = 1.0  # default GREEN ceiling for unlisted metrics

DTW_NORMALIZED_THRESHOLD = 0.15  # default DTW distance threshold


# ── Enums / data structures ─────────────────────────────────────

class Fidelity(str, Enum):
    GREEN = "GREEN"
    YELLOW = "YELLOW"
    RED = "RED"


@dataclass
class MetricDelta:
    """Original percentage-delta result for one metric."""
    metric: str
    values: list[float]
    labels: list[str]
    deltas: list[float]
    pct_deltas: list[float]
    divergent: list[bool]


@dataclass
class SigmaScore:
    """Sigma-scoring result for one metric."""
    metric: str
    sim_mean: float
    sim_std: float
    observed: float
    z: float
    fidelity: Fidelity
    tolerance: float  # GREEN ceiling in σ


@dataclass
class DTWResult:
    """Dynamic Time Warping result for a time-series metric."""
    metric: str
    distance: float
    normalized_distance: float
    threshold: float
    fidelity: Fidelity
    series_a_len: int
    series_b_len: int


@dataclass
class CompareResult:
    """Original percentage-delta comparison result."""
    tier_files: list[str]
    tier_labels: list[str]
    threshold: float
    metrics: list[MetricDelta]
    has_divergence: bool = False


@dataclass
class FidelityScorecard:
    """Distribution-aware fidelity scorecard."""
    sigma_scores: list[SigmaScore]
    dtw_results: list[DTWResult] = field(default_factory=list)
    overall: Fidelity = Fidelity.GREEN

    def to_dict(self) -> dict[str, Any]:
        scores = []
        for s in self.sigma_scores:
            scores.append({
                "metric": s.metric,
                "sim_mean": s.sim_mean,
                "sim_std": s.sim_std,
                "observed": s.observed,
                "z": round(s.z, 4),
                "fidelity": s.fidelity.value,
                "tolerance_sigma": s.tolerance,
            })
        dtw = []
        for d in self.dtw_results:
            dtw.append({
                "metric": d.metric,
                "distance": round(d.distance, 4),
                "normalized_distance": round(d.normalized_distance, 4),
                "threshold": d.threshold,
                "fidelity": d.fidelity.value,
            })
        return {
            "overall_fidelity": self.overall.value,
            "sigma_scores": scores,
            "dtw_results": dtw,
        }

    def to_json(self, indent: int = 2) -> str:
        return json.dumps(self.to_dict(), indent=indent)

    def to_markdown(self) -> str:
        lines = [
            "# Fidelity Scorecard",
            "",
            f"**Overall: {_fidelity_icon(self.overall)} {self.overall.value}**",
            "",
            "## Sigma Scores",
            "",
            "| Metric | Sim μ | Sim σ | Observed | z | Rating |",
            "|--------|------:|------:|---------:|--:|--------|",
        ]
        for s in self.sigma_scores:
            icon = _fidelity_icon(s.fidelity)
            lines.append(
                f"| {s.metric} | {s.sim_mean:.4f} | {s.sim_std:.4f} "
                f"| {s.observed:.4f} | {s.z:+.2f} | {icon} {s.fidelity.value} |"
            )
        if self.dtw_results:
            lines += [
                "",
                "## Time-Series (DTW)",
                "",
                "| Metric | Distance | Normalized | Threshold | Rating |",
                "|--------|--------:|-----------:|----------:|--------|",
            ]
            for d in self.dtw_results:
                icon = _fidelity_icon(d.fidelity)
                lines.append(
                    f"| {d.metric} | {d.distance:.4f} | {d.normalized_distance:.4f} "
                    f"| {d.threshold} | {icon} {d.fidelity.value} |"
                )
        lines.append("")
        return "\n".join(lines)


def _fidelity_icon(f: Fidelity) -> str:
    return {"GREEN": "🟢", "YELLOW": "🟡", "RED": "🔴"}[f.value]


# ── DTW implementation ───────────────────────────────────────────

def dtw_distance(a: Sequence[float], b: Sequence[float]) -> float:
    """Compute DTW distance between two sequences. O(n*m) DP."""
    n, m = len(a), len(b)
    if n == 0 or m == 0:
        return 0.0
    # Use flat array for memory efficiency
    prev = [math.inf] * (m + 1)
    curr = [math.inf] * (m + 1)
    prev[0] = 0.0
    for i in range(1, n + 1):
        curr[0] = math.inf
        for j in range(1, m + 1):
            cost = abs(a[i - 1] - b[j - 1])
            curr[j] = cost + min(prev[j], curr[j - 1], prev[j - 1])
        prev, curr = curr, prev
    return prev[m]


# ── Sigma scoring ────────────────────────────────────────────────

def _rate_z(z: float, tolerance: float) -> Fidelity:
    """Rate a z-score: |z| < tolerance → GREEN, < 2*tolerance → YELLOW, else RED."""
    az = abs(z)
    if az < tolerance:
        return Fidelity.GREEN
    if az < tolerance * 2:
        return Fidelity.YELLOW
    return Fidelity.RED


def compute_sigma_scores(
    sim_runs: pl.DataFrame,
    observed: pl.DataFrame,
    metrics: Sequence[str] = COMPARE_METRICS,
    tolerances: dict[str, float] | None = None,
) -> list[SigmaScore]:
    """Compute z-scores for each metric.

    Args:
        sim_runs: Tier 1 DataFrame with one row per seed run.
        observed: Tier 2 DataFrame (single or few rows, mean is taken).
        metrics: Columns to score.
        tolerances: Per-metric GREEN ceiling in σ. Falls back to DEFAULT_TOLERANCES
                    then DEFAULT_SIGMA_THRESHOLD.
    """
    tol = {**DEFAULT_TOLERANCES, **(tolerances or {})}
    scores: list[SigmaScore] = []
    for m in metrics:
        if m not in sim_runs.columns or m not in observed.columns:
            continue
        sim_mean = sim_runs[m].mean() or 0.0
        sim_std = sim_runs[m].std() or 0.0
        obs_val = observed[m].mean() or 0.0
        sigma = tol.get(m, DEFAULT_SIGMA_THRESHOLD)
        z = (obs_val - sim_mean) / sim_std if sim_std > 1e-12 else 0.0
        scores.append(SigmaScore(
            metric=m, sim_mean=sim_mean, sim_std=sim_std,
            observed=obs_val, z=z, fidelity=_rate_z(z, sigma), tolerance=sigma,
        ))
    return scores


def compute_dtw(
    sim_series: Sequence[float],
    obs_series: Sequence[float],
    metric: str = "node_count_over_time",
    threshold: float = DTW_NORMALIZED_THRESHOLD,
) -> DTWResult:
    """Compare two time series via DTW and rate fidelity."""
    dist = dtw_distance(sim_series, obs_series)
    length = max(len(sim_series), len(obs_series), 1)
    norm = dist / length
    if norm < threshold:
        fidelity = Fidelity.GREEN
    elif norm < threshold * 2:
        fidelity = Fidelity.YELLOW
    else:
        fidelity = Fidelity.RED
    return DTWResult(
        metric=metric, distance=dist, normalized_distance=norm,
        threshold=threshold, fidelity=fidelity,
        series_a_len=len(sim_series), series_b_len=len(obs_series),
    )


def build_scorecard(
    sigma_scores: list[SigmaScore],
    dtw_results: list[DTWResult] | None = None,
) -> FidelityScorecard:
    """Build a FidelityScorecard and compute overall rating."""
    dtw_results = dtw_results or []
    all_fidelities = [s.fidelity for s in sigma_scores] + [d.fidelity for d in dtw_results]
    if any(f == Fidelity.RED for f in all_fidelities):
        overall = Fidelity.RED
    elif any(f == Fidelity.YELLOW for f in all_fidelities):
        overall = Fidelity.YELLOW
    else:
        overall = Fidelity.GREEN
    return FidelityScorecard(
        sigma_scores=sigma_scores, dtw_results=dtw_results, overall=overall,
    )


# ── Original percentage-delta comparison ─────────────────────────

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
        tier_files=[], tier_labels=list(labels), threshold=threshold, metrics=[],
    )
    for metric in metrics:
        means = []
        for df in dfs:
            means.append(df[metric].mean() if metric in df.columns else 0.0)
        baseline = means[0] if means[0] else 1e-9
        deltas = [m - means[0] for m in means]
        pct_deltas = [d / abs(baseline) for d in deltas]
        divergent = [abs(p) > threshold for p in pct_deltas]
        divergent[0] = False
        md = MetricDelta(
            metric=metric, values=means, labels=list(labels),
            deltas=deltas, pct_deltas=pct_deltas, divergent=divergent,
        )
        result.metrics.append(md)
        if any(divergent):
            result.has_divergence = True
    return result


# ── HTML report generation ───────────────────────────────────────

def _bar_chart_svg(md: MetricDelta, width: int = 400, height: int = 180) -> str:
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
        vals_html = "".join(f"<td>{md.values[i]:.4f}</td>" for i in range(len(md.values)))
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
            f"<div class='chart'><h3>{esc_metric}</h3>{_bar_chart_svg(md)}</div>"
        )
    tier_headers = "".join(f"<th>{html.escape(l)}</th>" for l in result.tier_labels)
    delta_headers = "".join(
        f"<th>Δ vs {html.escape(result.tier_labels[0])}</th>" for _ in result.tier_labels
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
        output.write_text(generate_html_report(result))
    return result


def compare_sigma(
    sim_runs: pl.DataFrame,
    observed: pl.DataFrame,
    metrics: Sequence[str] = COMPARE_METRICS,
    tolerances: dict[str, float] | None = None,
    ts_column: str | None = "node_count_over_time",
    dtw_threshold: float = DTW_NORMALIZED_THRESHOLD,
    output_json: Path | str | None = None,
    output_md: Path | str | None = None,
) -> FidelityScorecard:
    """Distribution-aware sigma scoring with optional DTW.

    Args:
        sim_runs: Tier 1 DataFrame (one row per seed).
        observed: Tier 2 DataFrame (one or few rows).
        metrics: Scalar metrics to sigma-score.
        tolerances: Per-metric GREEN ceiling in σ.
        ts_column: Column containing time-series list for DTW (None to skip).
        dtw_threshold: Normalized DTW distance threshold.
        output_json: Write scorecard JSON to this path.
        output_md: Write scorecard markdown to this path.

    Returns:
        FidelityScorecard with sigma scores, DTW results, and overall rating.
    """
    scores = compute_sigma_scores(sim_runs, observed, metrics, tolerances)
    dtw_results: list[DTWResult] = []
    if ts_column and ts_column in sim_runs.columns and ts_column in observed.columns:
        # Average the sim time-series element-wise, compare to observed mean series
        sim_series = _mean_list_column(sim_runs, ts_column)
        obs_series = _mean_list_column(observed, ts_column)
        if sim_series and obs_series:
            dtw_results.append(compute_dtw(sim_series, obs_series, ts_column, dtw_threshold))
    scorecard = build_scorecard(scores, dtw_results)
    if output_json:
        p = Path(output_json)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(scorecard.to_json())
    if output_md:
        p = Path(output_md)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(scorecard.to_markdown())
    return scorecard


def _mean_list_column(df: pl.DataFrame, col: str) -> list[float]:
    """Extract mean time-series from a list-typed column across rows."""
    series = df[col]
    if series.dtype == pl.List(pl.Float64) or str(series.dtype).startswith("List"):
        lists = series.to_list()
        valid = [lst for lst in lists if lst is not None]
        if not valid:
            return []
        max_len = max(len(lst) for lst in valid)
        result = []
        for i in range(max_len):
            vals = [lst[i] for lst in valid if i < len(lst)]
            result.append(sum(vals) / len(vals) if vals else 0.0)
        return result
    # Fallback: treat as scalar series
    return series.to_list()
