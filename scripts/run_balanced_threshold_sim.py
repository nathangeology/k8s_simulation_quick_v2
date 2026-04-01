#!/usr/bin/env python3
"""Run balanced-threshold-verify scenario across 5 variants and generate report.

Captures per-variant: node count timeseries, disruption counts per transition,
CostJustified/Balanced vs Empty vs Underutilized decision counts, final node count,
and consolidation_score distribution.

Usage:
    python scripts/run_balanced_threshold_sim.py
"""
import json
import os
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
REPO_ROOT = SCRIPT_DIR.parent
SCENARIO = REPO_ROOT / "scenarios" / "balanced-threshold-verify.yaml"
RESULTS_DIR = REPO_ROOT / "results" / "balanced-threshold-verify"

VARIANTS = ["when-empty", "balanced-k1", "balanced-k2", "balanced-k4", "when-underutilized"]
SEED = 42


def extract_window_disruptions(timeseries, time_mode="wall_clock"):
    """Extract disruption counts per measurement window from timeseries."""
    # wall_clock: times in nanoseconds
    t_15m = 15 * 60 * 1_000_000_000
    t_25m = 25 * 60 * 1_000_000_000
    t_40m = 40 * 60 * 1_000_000_000

    disruptions_before_15m = 0
    disruptions_at_25m = 0
    disruptions_at_40m = 0

    for snap in timeseries:
        t = snap["time"]
        d = snap["disruption_count"]
        if t <= t_15m:
            disruptions_before_15m = d
        if t <= t_25m:
            disruptions_at_25m = d
        if t <= t_40m:
            disruptions_at_40m = d

    return {
        "transition_500_350": disruptions_at_25m - disruptions_before_15m,
        "transition_350_10": disruptions_at_40m - disruptions_at_25m,
        "total": disruptions_at_40m,
    }


def extract_decision_counts(timeseries):
    """Extract consolidation decision counts from timeseries snapshots."""
    total = 0
    accepted = 0
    rejected = 0
    for snap in timeseries:
        total = max(total, snap.get("consolidation_decisions_total", 0))
        accepted = max(accepted, snap.get("consolidation_decisions_accepted", 0))
        rejected = max(rejected, snap.get("consolidation_decisions_rejected", 0))
    return {"total": total, "accepted": accepted, "rejected": rejected}


def extract_node_timeseries(timeseries):
    """Extract (time_minutes, node_count) pairs."""
    return [
        {"time_min": snap["time"] / 60e9, "nodes": snap["node_count"]}
        for snap in timeseries
    ]


def main():
    try:
        import kubesim
    except ImportError:
        print("kubesim not importable — build with: maturin develop --release", file=sys.stderr)
        sys.exit(1)

    yaml_str = SCENARIO.read_text()
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    sim = kubesim.Simulation(yaml_str, seed=SEED)
    all_results = {}

    for variant in VARIANTS:
        print(f"Running: {variant}...", end=" ", flush=True)
        result = sim.run(variant)
        ts = list(result.timeseries)

        disruptions = extract_window_disruptions(ts)
        decisions = extract_decision_counts(ts)
        node_ts = extract_node_timeseries(ts)

        summary = {
            "variant": variant,
            "final_node_count": result.node_count,
            "peak_node_count": result.peak_node_count,
            "disruption_count": result.disruption_count,
            "disruptions_per_transition": disruptions,
            "consolidation_decisions": decisions,
            "cumulative_cost": result.cumulative_cost,
            "time_weighted_node_count": result.time_weighted_node_count,
            "running_pods": result.running_pods,
            "pending_pods": result.pending_pods,
            "events_processed": result.events_processed,
        }

        vdir = RESULTS_DIR / variant
        vdir.mkdir(parents=True, exist_ok=True)
        with open(vdir / "sim-results.json", "w") as f:
            json.dump(summary, f, indent=2)
        with open(vdir / "node-timeseries.json", "w") as f:
            json.dump(node_ts, f, indent=2)

        all_results[variant] = summary
        print(f"nodes={result.node_count} disruptions={result.disruption_count} "
              f"CJ_accepted={decisions['accepted']} CJ_total={decisions['total']}")

    # Write combined results
    with open(RESULTS_DIR / "sim-all-results.json", "w") as f:
        json.dump(all_results, f, indent=2)

    # Print comparison table
    print("\n## Balanced Threshold Gradient — Simulator Results")
    print(f"{'Variant':<22} {'Final Nodes':>11} {'Peak Nodes':>11} {'Disruptions':>11} "
          f"{'500→350':>8} {'350→10':>8} {'CJ Accept':>10} {'CJ Total':>10}")
    print("-" * 105)
    for v in VARIANTS:
        r = all_results[v]
        d = r["disruptions_per_transition"]
        c = r["consolidation_decisions"]
        print(f"{v:<22} {r['final_node_count']:>11} {r['peak_node_count']:>11} "
              f"{r['disruption_count']:>11} {d['transition_500_350']:>8} "
              f"{d['transition_350_10']:>8} {c['accepted']:>10} {c['total']:>10}")

    # Check expected gradient
    node_counts = [all_results[v]["final_node_count"] for v in VARIANTS]
    print(f"\nNode count gradient: {' > '.join(f'{v}={n}' for v, n in zip(VARIANTS, node_counts))}")

    expected_order = all(node_counts[i] >= node_counts[i+1] for i in range(len(node_counts)-1))
    if expected_order:
        print("✅ Expected gradient: when-empty >= balanced-k1 >= balanced-k2 >= balanced-k4 >= when-underutilized")
    else:
        print("⚠️  Gradient not strictly monotonic (may be expected with stochastic sim)")

    print(f"\nResults written to: {RESULTS_DIR}")


if __name__ == "__main__":
    main()
