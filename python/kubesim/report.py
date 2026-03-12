"""A/B comparison report module for study variant results.

Takes batch_run results for a multi-variant study and produces structured
comparison output as JSON (machine-readable) and Markdown (human-readable).
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import polars as pl
from scipy import stats

from kubesim.analysis import results_to_df, bootstrap_ci

METRICS = [
    "cumulative_cost", "time_weighted_node_count", "time_to_stable",
    "cumulative_pending_pod_seconds", "disruption_count", "disruption_seconds",
    "peak_node_count", "peak_cost_rate",
]

# End-state metrics kept for diagnostics
DIAGNOSTIC_METRICS = [
    "total_cost_per_hour", "node_count", "running_pods",
    "pending_pods", "events_processed", "final_time",
]


def _variant_summary(df: pl.DataFrame, variant: str, metrics: list[str] | None = None) -> dict:
    """Mean/median/p90/p99 for each metric for one variant."""
    vdf = df.filter(pl.col("variant") == variant)
    metric_list = metrics if metrics is not None else METRICS
    summary = {}
    for m in metric_list:
        if m not in vdf.columns:
            continue
        vals = vdf[m].drop_nulls()
        summary[m] = {
            "mean": float(vals.mean()),
            "median": float(vals.median()),
            "p90": float(vals.quantile(0.90)),
            "p99": float(vals.quantile(0.99)),
        }
    return summary


def _comparison_table(df: pl.DataFrame, variant_a: str, variant_b: str, metrics: list[str] | None = None) -> list[dict]:
    """Per-metric comparison: winner, signed delta, % effect size, p-value."""
    metric_list = metrics if metrics is not None else METRICS
    rows = []
    a_df = df.filter(pl.col("variant") == variant_a)
    b_df = df.filter(pl.col("variant") == variant_b)
    for m in metric_list:
        if m not in df.columns:
            continue
        a_vals = a_df[m].drop_nulls().to_numpy()
        b_vals = b_df[m].drop_nulls().to_numpy()
        if len(a_vals) == 0 or len(b_vals) == 0:
            continue
        mean_a, mean_b = float(np.mean(a_vals)), float(np.mean(b_vals))
        delta = mean_a - mean_b
        denom = mean_b if mean_b != 0 else 1.0
        pct = delta / abs(denom) * 100
        _, p_value = stats.mannwhitneyu(a_vals, b_vals, alternative="two-sided")
        bci = bootstrap_ci(df, variant_a, variant_b, m, n_boot=5000)
        winner = variant_a if delta < 0 else variant_b if delta > 0 else "tie"
        # For cost/count metrics, lower is better
        rows.append({
            "metric": m,
            "winner": winner,
            "delta": round(delta, 6),
            "effect_pct": round(pct, 4),
            "p_value": round(float(p_value), 6),
            "ci_low": round(bci["ci_low"], 6),
            "ci_high": round(bci["ci_high"], 6),
        })
    return rows


def generate_report(results: list[dict], study_name: str) -> dict:
    """Build the full report structure from batch_run results."""
    df = results_to_df(results)
    variants = sorted(df["variant"].unique().to_list())

    report: dict = {
        "study": study_name,
        "variants": variants,
        "runs_per_variant": int(df.filter(pl.col("variant") == variants[0]).height),
        "per_variant": {v: _variant_summary(df, v) for v in variants},
        "per_variant_diagnostic": {v: _variant_summary(df, v, DIAGNOSTIC_METRICS) for v in variants},
    }

    if len(variants) == 2:
        report["comparison"] = _comparison_table(df, variants[0], variants[1])
        report["comparison_diagnostic"] = _comparison_table(df, variants[0], variants[1], DIAGNOSTIC_METRICS)

    return report


def report_to_markdown(report: dict) -> str:
    """Render report dict as human-readable Markdown."""
    lines = [
        f"# A/B Comparison Report: {report['study']}",
        "",
        f"Variants: {', '.join(report['variants'])}  ",
        f"Runs per variant: {report['runs_per_variant']}",
        "",
    ]

    # Per-variant cumulative metrics (primary)
    for v in report["variants"]:
        lines.append(f"## Variant: {v}")
        lines.append("")
        lines.append("### Cumulative Metrics (Primary)")
        lines.append("")
        lines.append("| Metric | Mean | Median | p90 | p99 |")
        lines.append("|--------|------|--------|-----|-----|")
        for m, s in report["per_variant"][v].items():
            lines.append(f"| {m} | {s['mean']:.4f} | {s['median']:.4f} | {s['p90']:.4f} | {s['p99']:.4f} |")
        lines.append("")

        # Diagnostic end-state metrics
        if "per_variant_diagnostic" in report and v in report["per_variant_diagnostic"]:
            lines.append("### End-State Metrics (Diagnostic)")
            lines.append("")
            lines.append("| Metric | Mean | Median | p90 | p99 |")
            lines.append("|--------|------|--------|-----|-----|")
            for m, s in report["per_variant_diagnostic"][v].items():
                lines.append(f"| {m} | {s['mean']:.4f} | {s['median']:.4f} | {s['p90']:.4f} | {s['p99']:.4f} |")
            lines.append("")

    # Comparison table (cumulative — primary)
    if "comparison" in report:
        lines.append("## Comparison (Cumulative Metrics)")
        lines.append("")
        lines.append("| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |")
        lines.append("|--------|--------|-------------|----------|---------|--------|")
        for r in report["comparison"]:
            ci = f"[{r['ci_low']:.4f}, {r['ci_high']:.4f}]"
            lines.append(
                f"| {r['metric']} | {r['winner']} | {r['delta']:.4f} | "
                f"{r['effect_pct']:.2f}% | {r['p_value']:.4g} | {ci} |"
            )
        lines.append("")

    # Comparison table (diagnostic — end-state)
    if "comparison_diagnostic" in report:
        lines.append("## Comparison (End-State Diagnostics)")
        lines.append("")
        lines.append("| Metric | Winner | Delta (A−B) | Effect % | p-value | 95% CI |")
        lines.append("|--------|--------|-------------|----------|---------|--------|")
        for r in report["comparison_diagnostic"]:
            ci = f"[{r['ci_low']:.4f}, {r['ci_high']:.4f}]"
            lines.append(
                f"| {r['metric']} | {r['winner']} | {r['delta']:.4f} | "
                f"{r['effect_pct']:.2f}% | {r['p_value']:.4g} | {ci} |"
            )
        lines.append("")

    return "\n".join(lines)


def run_report(scenario_path: str, seeds: int = 5, output_dir: str = "results") -> int:
    """Load scenario, run both variants, generate report."""
    import yaml
    from kubesim._native import batch_run

    path = Path(scenario_path)
    with open(path) as f:
        scenario = yaml.safe_load(f)

    study = scenario.get("study", scenario)
    study_name = study.get("name", path.stem)

    config_yaml = yaml.dump(scenario, default_flow_style=False)
    seed_list = list(range(seeds))
    raw = batch_run(config_yaml, seed_list)
    results = [dict(r) if not isinstance(r, dict) else r for r in raw]

    report = generate_report(results, study_name)
    md = report_to_markdown(report)

    out = Path(output_dir) / study_name
    out.mkdir(parents=True, exist_ok=True)
    (out / "report.json").write_text(json.dumps(report, indent=2))
    (out / "report.md").write_text(md)

    # Generate diagnostic plots if matplotlib is available
    try:
        from kubesim.plots import generate_plots
        plot_paths = generate_plots(results, out)
        if plot_paths:
            print(f"Generated {len(plot_paths)} diagnostic plots in {out}/")
    except ImportError:
        pass  # matplotlib not installed — skip plots

    print(f"Report written to {out}/report.json and {out}/report.md")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="A/B comparison report for study variants")
    parser.add_argument("scenario", help="Path to scenario YAML")
    parser.add_argument("--seeds", type=int, default=5, help="Number of seeds to run")
    parser.add_argument("--output-dir", default="results", help="Output directory")
    args = parser.parse_args(argv)
    return run_report(args.scenario, args.seeds, args.output_dir)
