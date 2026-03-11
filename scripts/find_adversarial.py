#!/usr/bin/env python3
"""Find adversarial scenarios where MostAllocated vs LeastAllocated diverges most.

Sweeps random cluster scenarios via Hypothesis strategies, injects both
scheduling variants, runs them through kubesim batch_run, and ranks by
a composite divergence metric covering cost, running pods, and pending pods.
Top 10 worst-case scenarios are saved as individual YAML files.
"""

import os
import sys

import yaml

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from kubesim._native import batch_run
from kubesim.strategies import cluster_scenario

BUDGET = 1000
TOP_K = 10
SEEDS = [42, 100, 200]
OUTPUT_DIR = os.path.join(os.path.dirname(__file__), "..", "scenarios", "adversarial")

VARIANTS = [
    {"name": "most_allocated", "scheduler": {"scoring": "MostAllocated", "weight": 1}},
    {"name": "least_allocated", "scheduler": {"scoring": "LeastAllocated", "weight": 1}},
]


def evaluate(scenario: dict) -> dict:
    """Run scenario with both variants, return divergence metrics."""
    study = scenario.get("study", scenario)
    study["variants"] = VARIANTS
    config_yaml = yaml.dump(scenario, default_flow_style=False)
    try:
        results = batch_run(config_yaml, SEEDS)
    except Exception:
        return {"score": 0.0, "cost_div": 0.0, "running_div": 0.0, "pending_div": 0.0}

    rows = [dict(r) if not isinstance(r, dict) else r for r in results]
    most = [r for r in rows if r["variant"] == "most_allocated"]
    least = [r for r in rows if r["variant"] == "least_allocated"]
    if not most or not least:
        return {"score": 0.0, "cost_div": 0.0, "running_div": 0.0, "pending_div": 0.0}

    def avg(lst, key):
        return sum(r[key] for r in lst) / len(lst)

    cost_div = abs(avg(most, "total_cost_per_hour") - avg(least, "total_cost_per_hour"))
    running_div = abs(avg(most, "running_pods") - avg(least, "running_pods"))
    pending_div = abs(avg(most, "pending_pods") - avg(least, "pending_pods"))
    node_div = abs(avg(most, "node_count") - avg(least, "node_count"))

    # Composite score: cost divergence + normalized scheduling efficiency gap
    score = cost_div + running_div * 0.01 + pending_div * 0.01 + node_div * 0.1

    return {
        "score": score,
        "cost_div": cost_div,
        "running_div": running_div,
        "pending_div": pending_div,
        "node_div": node_div,
        "most_cost": avg(most, "total_cost_per_hour"),
        "least_cost": avg(least, "total_cost_per_hour"),
        "most_running": avg(most, "running_pods"),
        "least_running": avg(least, "running_pods"),
        "most_pending": avg(most, "pending_pods"),
        "least_pending": avg(least, "pending_pods"),
    }


def main():
    from hypothesis import given, settings, HealthCheck

    strat = cluster_scenario(
        max_nodes=200,
        min_workloads=1,
        max_workloads=8,
        min_pools=1,
        max_pools=3,
    )

    scored = []
    counter = {"n": 0}

    @settings(
        max_examples=BUDGET,
        database=None,
        suppress_health_check=[HealthCheck.too_slow],
        derandomize=True,
    )
    @given(scenario=strat)
    def search(scenario):
        if counter["n"] >= BUDGET:
            return
        counter["n"] += 1
        metrics = evaluate(scenario)
        scored.append((metrics, scenario))
        if counter["n"] % 100 == 0:
            best = max(s[0]["score"] for s in scored)
            print(f"  evaluated {counter['n']}/{BUDGET}, best score: {best:.6f}")

    print(f"Running adversarial search (budget={BUDGET}, seeds={SEEDS})...")
    try:
        search()
    except Exception as e:
        if scored:
            print(f"  search stopped at {counter['n']} evaluations: {type(e).__name__}")
        else:
            raise

    scored.sort(key=lambda x: x[0]["score"], reverse=True)
    top = scored[:TOP_K]

    os.makedirs(OUTPUT_DIR, exist_ok=True)

    print(f"\nEvaluated {counter['n']} scenarios.")
    print(f"\nTop {len(top)} worst-case divergences (MostAllocated vs LeastAllocated):\n")
    print(f"{'#':>3} {'Score':>8} {'CostΔ':>8} {'RunΔ':>6} {'PendΔ':>6} {'NodeΔ':>6}  File")
    print("-" * 70)

    for i, (m, scenario) in enumerate(top):
        scenario.get("study", scenario)["variants"] = VARIANTS
        fname = f"worst_case_{i+1:02d}.yaml"
        path = os.path.join(OUTPUT_DIR, fname)
        with open(path, "w") as f:
            yaml.dump(scenario, f, default_flow_style=False, sort_keys=False)
        print(f"{i+1:3d} {m['score']:8.4f} {m['cost_div']:8.4f} {m['running_div']:6.1f} {m['pending_div']:6.1f} {m['node_div']:6.1f}  {fname}")

    # Summary stats
    scores = [s[0]["score"] for s in scored]
    nonzero = [s for s in scores if s > 0]
    print(f"\nSummary:")
    print(f"  Total scenarios evaluated: {len(scores)}")
    print(f"  Scenarios with divergence > 0: {len(nonzero)}")
    if nonzero:
        print(f"  Max composite score: {max(nonzero):.6f}")
        print(f"  Mean score (nonzero): {sum(nonzero)/len(nonzero):.6f}")
        print(f"  Median score (nonzero): {sorted(nonzero)[len(nonzero)//2]:.6f}")

    if top and top[0][0]["score"] > 0:
        m = top[0][0]
        print(f"\n  Worst-case scenario details:")
        print(f"    MostAllocated:  cost={m['most_cost']:.4f}/hr, running={m['most_running']:.0f}, pending={m['most_pending']:.0f}")
        print(f"    LeastAllocated: cost={m['least_cost']:.4f}/hr, running={m['least_running']:.0f}, pending={m['least_pending']:.0f}")

    print(f"\nYAML files saved to: {os.path.abspath(OUTPUT_DIR)}/")


if __name__ == "__main__":
    main()
