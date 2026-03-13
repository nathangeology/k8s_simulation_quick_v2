#!/usr/bin/env python3
"""Compare real KIND+Karpenter results against kubesim distribution."""

import json
import sys
import os
import numpy as np

def load_real_timeseries(path):
    with open(path) as f:
        return json.load(f)

def load_sim_results(scenario_yaml, n_seeds=100):
    """Run kubesim with multiple seeds and return all results."""
    from kubesim._native import batch_run
    seeds = list(range(42, 42 + n_seeds))
    return batch_run(scenario_yaml, seeds, None)

def compute_cumulative_from_timeseries(ts):
    """Compute cumulative metrics from timeseries snapshots."""
    if not ts:
        return {}

    total_cost = 0.0
    total_vcpu_hours = 0.0
    total_mem_gib_hours = 0.0
    peak_nodes = 0
    peak_cost = 0.0

    for i in range(1, len(ts)):
        dt_hours = (ts[i]["elapsed_s"] - ts[i-1]["elapsed_s"]) / 3600
        total_cost += ts[i]["total_cost_per_hour"] * dt_hours
        total_vcpu_hours += ts[i]["total_vcpu_allocated"] * dt_hours
        total_mem_gib_hours += ts[i]["total_memory_allocated_gib"] * dt_hours
        peak_nodes = max(peak_nodes, ts[i]["node_count"])
        peak_cost = max(peak_cost, ts[i]["total_cost_per_hour"])

    return {
        "cumulative_cost": total_cost,
        "cumulative_vcpu_hours": total_vcpu_hours,
        "cumulative_mem_gib_hours": total_mem_gib_hours,
        "peak_nodes": peak_nodes,
        "peak_cost_per_hour": peak_cost,
        "end_nodes": ts[-1]["node_count"],
        "end_pods": ts[-1]["pod_count"],
        "end_cost_per_hour": ts[-1]["total_cost_per_hour"],
    }

def percentile_rank(distribution, value):
    """What percentile does `value` fall at in the distribution?"""
    return np.searchsorted(np.sort(distribution), value) / len(distribution) * 100

def main():
    if len(sys.argv) < 3:
        print("Usage: compare-results.py <real-timeseries.json> <scenario.yaml> [n_seeds]")
        sys.exit(1)

    real_path = sys.argv[1]
    scenario_yaml = sys.argv[2]
    n_seeds = int(sys.argv[3]) if len(sys.argv) > 3 else 100

    print(f"Loading real results from {real_path}...")
    real_ts = load_real_timeseries(real_path)
    real_metrics = compute_cumulative_from_timeseries(real_ts)

    print(f"Running kubesim with {n_seeds} seeds...")
    sim_results = load_sim_results(scenario_yaml, n_seeds)

    # Extract sim cumulative metrics
    sim_costs = [r["cumulative_cost"] for r in sim_results]
    sim_vcpu_hours = [r["cumulative_vcpu_hours"] for r in sim_results]
    sim_peak_nodes = [r["peak_node_count"] for r in sim_results]
    sim_end_nodes = [r["node_count"] for r in sim_results]
    sim_end_cost = [r["total_cost_per_hour"] for r in sim_results]

    # Report
    print("\n" + "="*70)
    print("KUBESIM VALIDATION REPORT: Real vs Simulated")
    print("="*70)

    print(f"\nReal cluster timeseries: {len(real_ts)} snapshots, {real_ts[-1]['elapsed_s']}s duration")
    print(f"Sim distribution: {n_seeds} seeds")

    metrics = [
        ("Cumulative Cost ($)", real_metrics["cumulative_cost"], sim_costs),
        ("Peak Nodes", real_metrics["peak_nodes"], sim_peak_nodes),
        ("End-State Nodes", real_metrics["end_nodes"], sim_end_nodes),
        ("End-State Cost ($/hr)", real_metrics["end_cost_per_hour"], sim_end_cost),
    ]

    print(f"\n{'Metric':<30} {'Real':>10} {'Sim Mean':>10} {'Sim p5':>10} {'Sim p95':>10} {'Pctl Rank':>10}")
    print("-"*80)
    for name, real_val, sim_dist in metrics:
        sim_arr = np.array(sim_dist)
        pctl = percentile_rank(sim_arr, real_val)
        print(f"{name:<30} {real_val:>10.2f} {sim_arr.mean():>10.2f} {np.percentile(sim_arr, 5):>10.2f} {np.percentile(sim_arr, 95):>10.2f} {pctl:>9.1f}%")

    # Fidelity gaps
    print("\n" + "="*70)
    print("FIDELITY GAP ANALYSIS")
    print("="*70)

    gaps = []
    for name, real_val, sim_dist in metrics:
        sim_mean = np.mean(sim_dist)
        pct_diff = abs(real_val - sim_mean) / max(sim_mean, 0.01) * 100
        pctl = percentile_rank(np.array(sim_dist), real_val)
        if pctl < 5 or pctl > 95:
            gaps.append((name, real_val, sim_mean, pct_diff, pctl, "OUTLIER"))
        elif pct_diff > 20:
            gaps.append((name, real_val, sim_mean, pct_diff, pctl, "LARGE_DIFF"))
        else:
            gaps.append((name, real_val, sim_mean, pct_diff, pctl, "OK"))

    for name, real_val, sim_mean, pct_diff, pctl, status in gaps:
        icon = "✅" if status == "OK" else "⚠️" if status == "LARGE_DIFF" else "❌"
        print(f"  {icon} {name}: real={real_val:.2f}, sim_mean={sim_mean:.2f} ({pct_diff:.1f}% diff, p{pctl:.0f})")

    # Save report
    report_dir = os.path.dirname(real_path)
    report = {
        "real_metrics": real_metrics,
        "sim_distribution": {
            "n_seeds": n_seeds,
            "cumulative_cost": {"mean": float(np.mean(sim_costs)), "p5": float(np.percentile(sim_costs, 5)), "p95": float(np.percentile(sim_costs, 95))},
            "peak_nodes": {"mean": float(np.mean(sim_peak_nodes)), "p5": float(np.percentile(sim_peak_nodes, 5)), "p95": float(np.percentile(sim_peak_nodes, 95))},
            "end_nodes": {"mean": float(np.mean(sim_end_nodes)), "p5": float(np.percentile(sim_end_nodes, 5)), "p95": float(np.percentile(sim_end_nodes, 95))},
        },
        "gaps": [{"metric": g[0], "real": g[1], "sim_mean": g[2], "pct_diff": g[3], "percentile": g[4], "status": g[5]} for g in gaps],
    }
    report_path = os.path.join(report_dir, "comparison.json")
    with open(report_path, "w") as f:
        json.dump(report, f, indent=2)
    print(f"\nReport saved to {report_path}")

if __name__ == "__main__":
    main()
