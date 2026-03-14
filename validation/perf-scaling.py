#!/usr/bin/env python3
"""Performance scaling audit: measure batch_run time across pod counts and constraint types.

Uses multiprocessing with hard kill for timeouts (signal.SIGALRM cannot interrupt native FFI calls).
"""

import multiprocessing as mp
import os
import sys
import time

import yaml

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

TIMEOUT_S = 30
POD_COUNTS = [50, 100, 250, 500, 1000, 2000]

CONSTRAINT_TYPES = {
    "none": {},
    "spread": {
        "topology_spread": {"max_skew": 1, "topology_key": "kubernetes.io/hostname"},
    },
    "antiaffinity": {
        "labels": {"app": "perf-test"},
        "pod_anti_affinity": {
            "label_key": "app",
            "topology_key": "kubernetes.io/hostname",
            "affinity_type": "required",
        },
    },
    "mixed": {
        "topology_spread": {"max_skew": 1, "topology_key": "kubernetes.io/hostname"},
        "labels": {"app": "perf-test"},
        "pod_anti_affinity": {
            "label_key": "app",
            "topology_key": "kubernetes.io/hostname",
            "affinity_type": "required",
        },
    },
}


def make_scenario(pod_count, constraint_extra):
    workload = {
        "type": "web_app",
        "count": 1,
        "replicas": {"fixed": pod_count},
        "cpu_request": {"dist": "uniform", "min": "250m", "max": "250m"},
        "memory_request": {"dist": "uniform", "min": "256Mi", "max": "256Mi"},
    }
    workload.update(constraint_extra)
    return yaml.dump({
        "study": {
            "name": "perf-scaling",
            "runs": 1,
            "time_mode": "wall_clock",
            "catalog_provider": "kwok",
            "cluster": {
                "node_pools": [{
                    "instance_types": ["c-4x-amd64-linux", "c-8x-amd64-linux",
                                       "c-16x-amd64-linux", "c-32x-amd64-linux"],
                    "min_nodes": 0,
                    "max_nodes": pod_count,
                    "karpenter": {"consolidation": {"policy": "WhenUnderutilized"}},
                }],
                "daemonsets": [{"name": "kube-proxy", "cpu_request": "100m", "memory_request": "256Mi"}],
                "delays": {
                    "node_startup": "30s", "node_startup_jitter": "5s",
                    "node_shutdown": "5s", "provisioner_batch": "10s",
                    "provisioner_batch_jitter": "3s", "pod_startup": "2s",
                },
            },
            "workloads": [workload],
            "variants": [{"name": "baseline", "scheduler": {"scoring": "LeastAllocated", "weight": 1}}],
        }
    }, default_flow_style=False)


def _run_in_subprocess(config_yaml, seeds, result_queue):
    from kubesim._native import batch_run
    results = batch_run(config_yaml, seeds)
    result_queue.put(results)


def run_with_timeout(config_yaml, seeds, timeout_s=TIMEOUT_S):
    q = mp.Queue()
    p = mp.Process(target=_run_in_subprocess, args=(config_yaml, seeds, q))
    p.start()
    p.join(timeout=timeout_s)
    if p.is_alive():
        p.kill()
        p.join()
        return None
    if q.empty():
        return None
    return q.get()


def main():
    print("Performance Scaling Audit")
    print("=" * 90)
    print(f"Timeout: {TIMEOUT_S}s | Pod counts: {POD_COUNTS}")
    print(f"Constraint types: {list(CONSTRAINT_TYPES.keys())}")
    print()

    # results[pod_count][constraint_name] = ms_per_seed or "TIMEOUT" or "ERROR"
    results = {}

    for pod_count in POD_COUNTS:
        seeds = list(range(42, 42 + (3 if pod_count <= 100 else 1)))
        results[pod_count] = {}
        for cname, cextra in CONSTRAINT_TYPES.items():
            config_yaml = make_scenario(pod_count, cextra)
            t0 = time.monotonic()
            out = run_with_timeout(config_yaml, seeds)
            elapsed = time.monotonic() - t0
            if out is None:
                results[pod_count][cname] = "TIMEOUT"
                print(f"  {pod_count:>5} pods / {cname:<12} => TIMEOUT ({elapsed:.1f}s)")
            else:
                ms_per_seed = (elapsed / len(seeds)) * 1000
                results[pod_count][cname] = ms_per_seed
                print(f"  {pod_count:>5} pods / {cname:<12} => {ms_per_seed:.1f} ms/seed ({len(seeds)} seeds, {elapsed:.1f}s total)")

    # Print summary table
    cnames = list(CONSTRAINT_TYPES.keys())
    print()
    print("=" * 90)
    print("SUMMARY TABLE")
    print("=" * 90)
    header = f"{'Pods':>6}" + "".join(f" | {c:>14}" for c in cnames) + " | Notes"
    print(header)
    print("-" * len(header))
    for pod_count in POD_COUNTS:
        row = f"{pod_count:>6}"
        notes = []
        for c in cnames:
            v = results[pod_count][c]
            if v == "TIMEOUT":
                row += f" | {'TIMEOUT':>14}"
                notes.append(f"{c}=TIMEOUT")
            else:
                row += f" | {v:>11.1f} ms"
        row += f" | {'; '.join(notes)}" if notes else " |"
        print(row)

    print()
    print("Done.")


if __name__ == "__main__":
    main()
