"""Polars-based analysis and reporting helpers for KubeSim batch_run results.

Compare variants (cost, disruption, latency distributions), run statistical
tests (Mann-Whitney U, bootstrap CI), generate plots (plotly), and produce
summary reports (HTML/markdown).
"""

from __future__ import annotations

from typing import Sequence

import polars as pl
from scipy import stats
import numpy as np

# ── DataFrame construction ───────────────────────────────────────

RESULT_COLUMNS = [
    "seed", "variant", "events_processed", "total_cost_per_hour",
    "node_count", "pod_count", "running_pods", "pending_pods", "final_time",
    "cumulative_cost", "time_weighted_node_count", "time_to_stable",
    "cumulative_pending_pod_seconds", "disruption_count", "disruption_seconds",
    "peak_node_count", "peak_cost_rate",
]


def results_to_df(results: list[dict] | list) -> pl.DataFrame:
    """Convert batch_run output (list of dicts or SimResult objects) to a Polars DataFrame."""
    if not results:
        return pl.DataFrame(schema={c: pl.Utf8 if c == "variant" else pl.Float64 for c in RESULT_COLUMNS})
    first = results[0]
    rows = [r if isinstance(r, dict) else r.to_dict() for r in results]
    return pl.DataFrame(rows)


# ── Variant comparison ───────────────────────────────────────────

_METRIC_COLS = [
    "cumulative_cost", "time_weighted_node_count", "time_to_stable",
    "cumulative_pending_pod_seconds", "disruption_count", "disruption_seconds",
    "peak_node_count", "peak_cost_rate",
    "total_cost_per_hour", "node_count", "pod_count",
    "running_pods", "pending_pods", "final_time",
]


def compare_variants(df: pl.DataFrame, metrics: Sequence[str] | None = None) -> pl.DataFrame:
    """Summary statistics per variant for the given metrics."""
    cols = list(metrics) if metrics else _METRIC_COLS
    aggs = []
    for c in cols:
        aggs.extend([
            pl.col(c).mean().alias(f"{c}_mean"),
            pl.col(c).std().alias(f"{c}_std"),
            pl.col(c).median().alias(f"{c}_median"),
            pl.col(c).quantile(0.05).alias(f"{c}_p5"),
            pl.col(c).quantile(0.95).alias(f"{c}_p95"),
        ])
    return df.group_by("variant").agg(aggs).sort("variant")


# ── Statistical tests ────────────────────────────────────────────

def mann_whitney(
    df: pl.DataFrame,
    variant_a: str,
    variant_b: str,
    metric: str = "total_cost_per_hour",
) -> dict:
    """Two-sided Mann-Whitney U test between two variants on a metric."""
    a = df.filter(pl.col("variant") == variant_a)[metric].to_numpy()
    b = df.filter(pl.col("variant") == variant_b)[metric].to_numpy()
    stat, p = stats.mannwhitneyu(a, b, alternative="two-sided")
    return {"statistic": float(stat), "p_value": float(p), "metric": metric,
            "variant_a": variant_a, "variant_b": variant_b, "n_a": len(a), "n_b": len(b)}


def bootstrap_ci(
    df: pl.DataFrame,
    variant_a: str,
    variant_b: str,
    metric: str = "total_cost_per_hour",
    n_boot: int = 10_000,
    ci: float = 0.95,
    seed: int = 42,
) -> dict:
    """Bootstrap confidence interval for the mean difference (a - b)."""
    rng = np.random.default_rng(seed)
    a = df.filter(pl.col("variant") == variant_a)[metric].to_numpy()
    b = df.filter(pl.col("variant") == variant_b)[metric].to_numpy()
    diffs = np.empty(n_boot)
    for i in range(n_boot):
        diffs[i] = rng.choice(a, len(a), replace=True).mean() - rng.choice(b, len(b), replace=True).mean()
    alpha = (1 - ci) / 2
    lo, hi = float(np.quantile(diffs, alpha)), float(np.quantile(diffs, 1 - alpha))
    return {"mean_diff": float(np.mean(diffs)), "ci_low": lo, "ci_high": hi,
            "ci": ci, "n_boot": n_boot, "metric": metric,
            "variant_a": variant_a, "variant_b": variant_b}


# ── Plotting helpers (plotly) ────────────────────────────────────

def plot_cost_over_time(df: pl.DataFrame) -> "plotly.graph_objects.Figure":
    """Box plot of total_cost_per_hour by variant."""
    import plotly.express as px
    return px.box(df.to_pandas(), x="variant", y="total_cost_per_hour",
                  title="Cost per Hour by Variant", points="outliers")


def plot_disruption_heatmap(df: pl.DataFrame, metric: str = "pending_pods") -> "plotly.graph_objects.Figure":
    """Heatmap of a metric across seeds and variants."""
    import plotly.express as px
    pivot = df.pivot(on="variant", index="seed", values=metric).to_pandas().set_index("seed")
    return px.imshow(pivot, title=f"{metric} by Seed × Variant",
                     labels=dict(x="Variant", y="Seed", color=metric), aspect="auto")


def plot_scheduling_latency_cdf(df: pl.DataFrame, metric: str = "final_time") -> "plotly.graph_objects.Figure":
    """Empirical CDF of a metric per variant."""
    import plotly.graph_objects as go
    fig = go.Figure()
    for variant in df["variant"].unique().sort().to_list():
        vals = np.sort(df.filter(pl.col("variant") == variant)[metric].to_numpy())
        cdf = np.arange(1, len(vals) + 1) / len(vals)
        fig.add_trace(go.Scatter(x=vals, y=cdf, mode="lines", name=variant))
    fig.update_layout(title=f"{metric} CDF by Variant", xaxis_title=metric, yaxis_title="CDF")
    return fig


# ── Report generation ────────────────────────────────────────────

def summary_report(df: pl.DataFrame, fmt: str = "markdown") -> str:
    """Generate a summary report comparing all variants.

    Args:
        df: batch_run results as a Polars DataFrame.
        fmt: 'markdown' or 'html'.
    """
    variants = sorted(df["variant"].unique().to_list())
    comp = compare_variants(df)

    lines: list[str] = []
    lines.append("# KubeSim Batch Run Summary")
    lines.append(f"\nRuns per variant: {df.filter(pl.col('variant') == variants[0]).height}")
    lines.append(f"Variants: {', '.join(variants)}\n")

    # Stats table
    lines.append("## Summary Statistics\n")
    lines.append(_df_to_md_table(comp))

    # Pairwise tests (if exactly 2 variants)
    if len(variants) == 2:
        a, b = variants
        lines.append(f"\n## Statistical Comparison: {a} vs {b}\n")
        for metric in ["cumulative_cost", "time_weighted_node_count", "time_to_stable",
                        "cumulative_pending_pod_seconds", "disruption_count",
                        "total_cost_per_hour", "node_count", "pending_pods"]:
            mw = mann_whitney(df, a, b, metric)
            bci = bootstrap_ci(df, a, b, metric)
            lines.append(f"### {metric}")
            lines.append(f"- Mann-Whitney U: stat={mw['statistic']:.1f}, p={mw['p_value']:.4g}")
            lines.append(f"- Mean diff ({a} − {b}): {bci['mean_diff']:.4f} "
                         f"[{bci['ci_low']:.4f}, {bci['ci_high']:.4f}] ({int(bci['ci']*100)}% CI)\n")

    md = "\n".join(lines)
    if fmt == "html":
        try:
            import markdown
            return markdown.markdown(md, extensions=["tables"])
        except ImportError:
            return f"<pre>{md}</pre>"
    return md


def _df_to_md_table(df: pl.DataFrame) -> str:
    """Convert a small Polars DataFrame to a markdown table."""
    cols = df.columns
    header = "| " + " | ".join(cols) + " |"
    sep = "| " + " | ".join("---" for _ in cols) + " |"
    rows = []
    for row in df.iter_rows():
        cells = []
        for v in row:
            cells.append(f"{v:.4f}" if isinstance(v, float) else str(v))
        rows.append("| " + " | ".join(cells) + " |")
    return "\n".join([header, sep] + rows)
