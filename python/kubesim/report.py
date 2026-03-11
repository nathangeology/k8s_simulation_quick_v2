"""A/B comparison report module for study variant results.

Takes batch_run results for a multi-variant study and produces structured
comparison output as JSON (machine-readable) and Markdown (human-readable).
"""

from __future__ import annotations

import json
import statistics
from pathlib import Path
from typing import Any


METRICS = [
    "total_cost_per_hour",
    "node_count",
    "running_pods",
    "pending_pods",
    "events_processed",
    "final_time",
]


def _variant_summary(values: list[float]) -> dict[str, float]:
    """Compute mean/median/p90/p99 for a list of metric values."""
    s = sorted(values)
    n = len(s)
    return {
        "mean": statistics.mean(s),
        "median": statistics.median(s),
        "p90": s[int(n * 0.9)] if n > 1 else s[0],
        "p99": s[int(n * 0.99)] if n > 1 else s[0],
    }


def _mann_whitney_p(a: list[float], b: list[float]) -> float | None:
    """Mann-Whitney U p-value, or None if scipy unavailable or samples too small."""
    if len(a) < 2 or len(b) < 2:
        return None
    try:
        from scipy.stats import mannwhitneyu
        _, p = mannwhitneyu(a, b, alternative="two-sided")
        return p
    except (ImportError, ValueError):
        return None


def generate_report(results: list[dict], study_name: str = "study") -> dict[str, Any]:
    """Build structured report data from batch_run results.

    Args:
        results: list of result dicts with variant + seed fields.
        study_name: name for the report header.

    Returns:
        Report dict with per_variant summaries and comparison table.
    """
    # Group by variant
    by_variant: dict[str, list[dict]] = {}
    for r in results:
        by_variant.setdefault(r["variant"], []).append(r)

    variants = sorted(by_variant)

    # Per-variant summaries
    per_variant: dict[str, dict] = {}
    for v in variants:
        rows = by_variant[v]
        per_variant[v] = {
            m: _variant_summary([r[m] for r in rows]) for m in METRICS
        }

    # Comparison table (pairwise for first two variants)
    comparison = []
    if len(variants) >= 2:
        va, vb = variants[0], variants[1]
        for m in METRICS:
            a_vals = [r[m] for r in by_variant[va]]
            b_vals = [r[m] for r in by_variant[vb]]
            mean_a, mean_b = statistics.mean(a_vals), statistics.mean(b_vals)
            delta = mean_a - mean_b
            pct = (delta / mean_b * 100) if mean_b != 0 else None
            p = _mann_whitney_p(a_vals, b_vals)
            winner = va if delta < 0 else vb if delta > 0 else "tie"
            comparison.append({
                "metric": m,
                "winner": winner,
                "delta": delta,
                "effect_pct": pct,
                "p_value": p,
            })

    return {
        "study": study_name,
        "variants": variants,
        "seeds": sorted({r["seed"] for r in results}),
        "per_variant": per_variant,
        "comparison": comparison,
    }


def report_to_json(report: dict) -> str:
    """Serialize report to JSON string."""
    return json.dumps(report, indent=2, default=str)


def report_to_markdown(report: dict) -> str:
    """Render report as human-readable Markdown."""
    lines = [f"# A/B Comparison Report: {report['study']}", ""]
    lines.append(f"Variants: {', '.join(report['variants'])}")
    lines.append(f"Seeds: {len(report['seeds'])}")
    lines.append("")

    # Comparison table
    if report["comparison"]:
        va, vb = report["variants"][0], report["variants"][1]
        lines.append(f"## Comparison: {va} vs {vb}")
        lines.append("")
        lines.append("| Metric | Winner | Delta ({a}\u2212{b}) | Effect % | p-value |".format(a=va, b=vb))
        lines.append("|--------|--------|-------|----------|---------|")
        for c in report["comparison"]:
            pv = f"{c['p_value']:.4g}" if c["p_value"] is not None else "n/a"
            ep = f"{c['effect_pct']:.2f}%" if c["effect_pct"] is not None else "n/a"
            lines.append(f"| {c['metric']} | {c['winner']} | {c['delta']:.4f} | {ep} | {pv} |")
        lines.append("")

    # Per-variant summaries
    lines.append("## Per-Variant Summaries")
    lines.append("")
    for v in report["variants"]:
        lines.append(f"### {v}")
        lines.append("")
        lines.append("| Metric | Mean | Median | p90 | p99 |")
        lines.append("|--------|------|--------|-----|-----|")
        for m in METRICS:
            s = report["per_variant"][v][m]
            lines.append(f"| {m} | {s['mean']:.4f} | {s['median']:.4f} | {s['p90']:.4f} | {s['p99']:.4f} |")
        lines.append("")

    return "\n".join(lines)


def write_report(results: list[dict], study_name: str, output_dir: str = "results") -> None:
    """Generate and write JSON + Markdown reports to output_dir/<study_name>/."""
    report = generate_report(results, study_name)
    out = Path(output_dir) / study_name
    out.mkdir(parents=True, exist_ok=True)
    (out / "report.json").write_text(report_to_json(report))
    (out / "report.md").write_text(report_to_markdown(report))
    print(f"Reports written to {out}/")


def report_main(argv: list[str] | None = None) -> int:
    """CLI entry point: kubesim report <scenario.yaml> [--seeds N] [--output-dir results/]."""
    import argparse
    import yaml
    from kubesim._native import batch_run

    parser = argparse.ArgumentParser(description="Run study variants and generate A/B comparison report")
    parser.add_argument("scenario", help="Path to scenario YAML file")
    parser.add_argument("--seeds", type=int, default=5, help="Number of seeds to run (default: 5)")
    parser.add_argument("--output-dir", default="results", help="Output directory (default: results/)")
    args = parser.parse_args(argv)

    scenario_path = Path(args.scenario)
    with open(scenario_path) as f:
        scenario = yaml.safe_load(f)

    study_name = scenario.get("study", {}).get("name", scenario_path.stem)
    seeds = list(range(args.seeds))

    print(f"Running {study_name} with {args.seeds} seeds...")
    raw = batch_run(str(scenario_path), seeds)
    results = [dict(r) if not isinstance(r, dict) else r for r in raw]

    write_report(results, study_name, args.output_dir)
    return 0
