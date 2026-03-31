#!/usr/bin/env python3
"""run_pod_deletion_cost_sim.py — Run simulator for 5 pod-deletion-cost variants.

Loads each variant scenario, runs the simulator, and writes results to
results/pod-deletion-cost-verify/<variant>/sim-results.json.

Usage:
    python scripts/run_pod_deletion_cost_sim.py
"""
import json
import os
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
REPO_ROOT = SCRIPT_DIR.parent
SCENARIOS_DIR = REPO_ROOT / "scenarios" / "pod-deletion-cost-verify"
RESULTS_DIR = REPO_ROOT / "results" / "pod-deletion-cost-verify"

VARIANTS = ["no-cost", "low-cost", "mid-cost", "high-cost", "mixed-cost"]

def main():
    try:
        import kubesim
    except ImportError:
        print("kubesim not importable — skipping simulator runs", file=sys.stderr)
        print("Build with: cd k8s && maturin develop --release", file=sys.stderr)
        sys.exit(0)

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    all_results = {}

    for variant in VARIANTS:
        scenario_file = SCENARIOS_DIR / f"{variant}.yaml"
        if not scenario_file.exists():
            print(f"SKIP: {scenario_file} not found")
            continue

        yaml_str = scenario_file.read_text()
        print(f"Running simulator: {variant}...")

        try:
            result = kubesim.run_scenario(yaml_str)
            metrics = result.get("metrics", {})
            summary = {
                "variant": variant,
                "disruption_count": metrics.get("disruption_count", 0),
                "peak_node_count": metrics.get("peak_node_count", 0),
                "final_node_count": metrics.get("final_node_count", 0),
                "cumulative_cost": metrics.get("cumulative_cost", 0),
                "time_weighted_node_count": metrics.get("time_weighted_node_count", 0),
                "decision_ratio_sum": metrics.get("decision_ratio_sum", 0),
                "decisions_total": metrics.get("decisions_total", 0),
                "decisions_accepted": metrics.get("decisions_accepted", 0),
                "decisions_rejected": metrics.get("decisions_rejected", 0),
            }
        except Exception as e:
            print(f"  ERROR: {e}")
            summary = {"variant": variant, "error": str(e)}

        vdir = RESULTS_DIR / variant
        vdir.mkdir(parents=True, exist_ok=True)
        with open(vdir / "sim-results.json", "w") as f:
            json.dump(summary, f, indent=2)

        all_results[variant] = summary
        print(f"  disruptions={summary.get('disruption_count', 'N/A')}, "
              f"peak_nodes={summary.get('peak_node_count', 'N/A')}, "
              f"final_nodes={summary.get('final_node_count', 'N/A')}")

    # Write combined results
    with open(RESULTS_DIR / "sim-all-results.json", "w") as f:
        json.dump(all_results, f, indent=2)

    print(f"\nResults written to: {RESULTS_DIR}")

    # Print comparison table
    print("\n## Simulator Comparison")
    print(f"{'Variant':<15} {'Disruptions':>12} {'Peak Nodes':>12} {'Final Nodes':>12} {'DR Accepted':>12}")
    print("-" * 65)
    for v in VARIANTS:
        r = all_results.get(v, {})
        if "error" in r:
            print(f"{v:<15} {'ERROR':>12}")
        else:
            print(f"{v:<15} {r.get('disruption_count',0):>12.1f} "
                  f"{r.get('peak_node_count',0):>12} "
                  f"{r.get('final_node_count',0):>12} "
                  f"{r.get('decisions_accepted',0):>12}")


if __name__ == "__main__":
    main()
