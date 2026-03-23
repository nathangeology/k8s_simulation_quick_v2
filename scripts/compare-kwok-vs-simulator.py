#!/usr/bin/env python3
"""compare-kwok-vs-simulator.py — Compare kwok verification results against simulator predictions.

Loads both result sets, computes per-variant deltas, checks structural criteria,
and generates comparison-report.md.

Usage:
    python scripts/compare-kwok-vs-simulator.py \
        --sim-dir results/consolidate-when/benchmark-tradeoff \
        --kwok-dir results/kwok-verify \
        --output results/kwok-verify/comparison-report.md
"""
import argparse
import json
import os
import sys
from pathlib import Path

# Tolerances from plan §6.1
TOLERANCES = {
    "node_count": 0.15,       # ±15%
    "pods_evicted": 3,        # ±3 absolute
    "total_cost": 0.10,       # ±10%
    "peak_node_count": 0.05,  # ±5%
}

VARIANTS = [
    "when-empty",
    "when-underutilized",
    "cost-justified-0.25",
    "cost-justified-0.50",
    "cost-justified-0.75",
    "cost-justified-1.00",
    "cost-justified-1.50",
    "cost-justified-2.00",
    "cost-justified-3.00",
    "cost-justified-5.00",
]


def load_kwok_results(kwok_dir: Path) -> dict:
    """Load per-variant summary.json from kwok results."""
    results = {}
    for variant in VARIANTS:
        summary = kwok_dir / variant / "summary.json"
        if summary.exists():
            with open(summary) as f:
                results[variant] = json.load(f)
        else:
            results[variant] = None
    return results


def load_sim_results(sim_dir: Path) -> dict:
    """Load simulator results. Expects per-variant JSON or a combined file."""
    results = {}
    # Try combined results file first
    combined = sim_dir / "results.json"
    if combined.exists():
        with open(combined) as f:
            data = json.load(f)
        if isinstance(data, dict) and "variants" in data:
            for v in data["variants"]:
                results[v.get("name", v.get("variant", ""))] = v
            return results
        if isinstance(data, list):
            for v in data:
                results[v.get("name", v.get("variant", ""))] = v
            return results

    # Try per-variant files
    for variant in VARIANTS:
        for fname in [f"{variant}.json", f"{variant}/results.json", f"{variant}/summary.json"]:
            p = sim_dir / fname
            if p.exists():
                with open(p) as f:
                    results[variant] = json.load(f)
                break
    return results


def check_structural_criteria(kwok: dict) -> list[dict]:
    """Check structural success criteria from plan §6.2."""
    checks = []

    # 1. WhenEmpty zero-disruption
    we = kwok.get("when-empty")
    if we:
        evictions = we.get("pods_evicted", 0)
        checks.append({
            "criterion": "WhenEmpty zero-disruption",
            "pass": int(evictions) == 0,
            "detail": f"evictions={evictions}",
        })

    # 2. WhenEmptyOrUnderutilized most disruptive
    wu = kwok.get("when-underutilized")
    if wu:
        wu_evictions = int(wu.get("pods_evicted", 0))
        max_other = max(
            (int(kwok[v].get("pods_evicted", 0)) for v in VARIANTS if v != "when-underutilized" and kwok.get(v)),
            default=0,
        )
        checks.append({
            "criterion": "WhenEmptyOrUnderutilized most disruptive",
            "pass": wu_evictions >= max_other,
            "detail": f"wu_evictions={wu_evictions}, max_other={max_other}",
        })

    # 3. Disruption monotonicity for CostJustified variants
    cj_variants = [v for v in VARIANTS if v.startswith("cost-justified-") and kwok.get(v)]
    if len(cj_variants) >= 2:
        eviction_seq = [int(kwok[v].get("pods_evicted", 0)) for v in cj_variants]
        monotonic = all(a >= b for a, b in zip(eviction_seq, eviction_seq[1:]))
        checks.append({
            "criterion": "Disruption monotonicity (higher threshold → fewer evictions)",
            "pass": monotonic,
            "detail": f"evictions={eviction_seq}",
        })

    return checks


def compute_deltas(sim: dict, kwok: dict) -> list[dict]:
    """Compute per-variant metric deltas between simulator and kwok."""
    deltas = []
    for variant in VARIANTS:
        s, k = sim.get(variant), kwok.get(variant)
        if not s or not k:
            deltas.append({"variant": variant, "status": "missing_data"})
            continue

        row = {"variant": variant, "status": "ok", "metrics": {}}

        # Evictions: absolute tolerance
        s_ev = int(s.get("pods_evicted", s.get("evictions", 0)))
        k_ev = int(k.get("pods_evicted", 0))
        abs_diff = abs(s_ev - k_ev)
        row["metrics"]["pods_evicted"] = {
            "sim": s_ev, "kwok": k_ev, "diff": abs_diff,
            "within_tolerance": abs_diff <= TOLERANCES["pods_evicted"],
        }

        # Node count: relative tolerance
        for metric in ["final_node_count", "peak_node_count"]:
            s_val = s.get(metric, s.get("node_count", 0))
            k_val = k.get(metric, k.get("final_node_count", 0))
            if s_val and k_val:
                rel = abs(s_val - k_val) / max(s_val, k_val, 1)
                tol_key = "peak_node_count" if "peak" in metric else "node_count"
                row["metrics"][metric] = {
                    "sim": s_val, "kwok": k_val, "rel_diff": round(rel, 4),
                    "within_tolerance": rel <= TOLERANCES[tol_key],
                }

        deltas.append(row)
    return deltas


def generate_report(kwok: dict, sim: dict, deltas: list, checks: list, output: Path):
    """Generate comparison-report.md."""
    lines = [
        "# KWOK vs Simulator Comparison Report",
        "",
        f"Generated: {__import__('datetime').datetime.utcnow().isoformat()}Z",
        "",
        "## Structural Criteria",
        "",
        "| Criterion | Pass | Detail |",
        "|-----------|------|--------|",
    ]
    for c in checks:
        mark = "✅" if c["pass"] else "❌"
        lines.append(f"| {c['criterion']} | {mark} | {c['detail']} |")

    lines += [
        "",
        "## Per-Variant Eviction Comparison",
        "",
        "| Variant | Sim Evictions | KWOK Evictions | Δ | Within Tolerance |",
        "|---------|--------------|----------------|---|-----------------|",
    ]
    for d in deltas:
        if d["status"] == "missing_data":
            lines.append(f"| {d['variant']} | — | — | — | no data |")
            continue
        ev = d["metrics"].get("pods_evicted", {})
        mark = "✅" if ev.get("within_tolerance") else "❌"
        lines.append(f"| {d['variant']} | {ev.get('sim', '—')} | {ev.get('kwok', '—')} | {ev.get('diff', '—')} | {mark} |")

    lines += [
        "",
        "## Per-Variant Node Count Comparison",
        "",
        "| Variant | Sim Nodes | KWOK Nodes | Rel Δ | Within Tolerance |",
        "|---------|-----------|------------|-------|-----------------|",
    ]
    for d in deltas:
        if d["status"] == "missing_data":
            lines.append(f"| {d['variant']} | — | — | — | no data |")
            continue
        nc = d["metrics"].get("final_node_count", d["metrics"].get("peak_node_count", {}))
        mark = "✅" if nc.get("within_tolerance") else "❌"
        lines.append(f"| {d['variant']} | {nc.get('sim', '—')} | {nc.get('kwok', '—')} | {nc.get('rel_diff', '—')} | {mark} |")

    # Overall verdict
    all_structural = all(c["pass"] for c in checks) if checks else False
    lines += [
        "",
        "## Verdict",
        "",
        f"Structural criteria: {'ALL PASS ✅' if all_structural else 'FAILURES DETECTED ❌'}",
        "",
    ]

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(lines) + "\n")
    print(f"Report written to: {output}")


def main():
    parser = argparse.ArgumentParser(description="Compare kwok vs simulator results")
    parser.add_argument("--sim-dir", default="results/consolidate-when/benchmark-tradeoff",
                        help="Simulator results directory")
    parser.add_argument("--kwok-dir", default="results/kwok-verify",
                        help="KWOK verification results directory")
    parser.add_argument("--output", default="results/kwok-verify/comparison-report.md",
                        help="Output report path")
    args = parser.parse_args()

    sim_dir = Path(args.sim_dir)
    kwok_dir = Path(args.kwok_dir)
    output = Path(args.output)

    kwok = load_kwok_results(kwok_dir)
    sim = load_sim_results(sim_dir)

    available_kwok = sum(1 for v in kwok.values() if v)
    available_sim = sum(1 for v in sim.values() if v)
    print(f"Loaded {available_kwok} kwok variants, {available_sim} simulator variants")

    if available_kwok == 0:
        print("ERROR: No kwok results found. Run the verification first.", file=sys.stderr)
        sys.exit(1)

    checks = check_structural_criteria(kwok)
    deltas = compute_deltas(sim, kwok) if available_sim > 0 else []

    generate_report(kwok, sim, deltas, checks, output)

    # Exit code: 0 if all structural checks pass, 1 otherwise
    if checks and not all(c["pass"] for c in checks):
        sys.exit(1)


if __name__ == "__main__":
    main()
