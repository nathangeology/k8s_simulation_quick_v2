"""Standalone time-series metrics collector for KWOK/KIND clusters.

Polls cluster state at a configurable interval, captures Karpenter events,
computes scheduling latency, and exports to parquet.

Usage::

    python -m kubesim.validation.metrics_collector \
        --context kind-kubesim-abc123 --duration 300 --interval 30 --output metrics.parquet
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# Instance pricing (on-demand us-east-1) — shared with eks.py
_INSTANCE_COST_PER_HOUR: dict[str, float] = {
    "m5.large": 0.096, "m5.xlarge": 0.192, "m5.2xlarge": 0.384,
    "m5.4xlarge": 0.768, "m5.8xlarge": 1.536,
    "c5.large": 0.085, "c5.xlarge": 0.170, "c5.2xlarge": 0.340,
    "c5.4xlarge": 0.680,
    "m6i.large": 0.096, "m6i.xlarge": 0.192, "m6i.2xlarge": 0.384,
    "r5.large": 0.126, "r5.xlarge": 0.252, "r5.2xlarge": 0.504,
    "t3.medium": 0.0416, "t3.large": 0.0832, "t3.xlarge": 0.1664,
}

_KARPENTER_REASONS = frozenset({
    "Provisioning", "Deprovisioning", "Consolidation",
    "Disruption", "Drift", "Expiration", "Emptiness",
})


def _kubectl_json(context: str, *args: str) -> dict[str, Any]:
    r = subprocess.run(
        ["kubectl", "--context", context, "-o", "json", *args],
        capture_output=True, text=True, check=True,
    )
    return json.loads(r.stdout)


def _parse_ts(ts: str | None) -> float:
    """Parse K8s RFC3339 timestamp to epoch seconds."""
    if not ts:
        return 0.0
    ts = ts.rstrip("Z")
    for fmt in ("%Y-%m-%dT%H:%M:%S.%f", "%Y-%m-%dT%H:%M:%S"):
        try:
            return datetime.strptime(ts, fmt).replace(tzinfo=timezone.utc).timestamp()
        except ValueError:
            continue
    return 0.0


@dataclass
class MetricsSnapshot:
    """Single point-in-time cluster snapshot."""
    timestamp: float
    node_count: int = 0
    running_pods: int = 0
    pending_pods: int = 0
    cost_per_hour: float = 0.0
    disruption_events: int = 0


def snapshot(context: str) -> MetricsSnapshot:
    """Collect a single metrics snapshot from the cluster."""
    ts = time.time()
    snap = MetricsSnapshot(timestamp=ts)

    # Nodes + cost
    nodes = _kubectl_json(context, "get", "nodes").get("items", [])
    snap.node_count = len(nodes)
    for node in nodes:
        itype = node.get("metadata", {}).get("labels", {}).get(
            "node.kubernetes.io/instance-type", "")
        snap.cost_per_hour += _INSTANCE_COST_PER_HOUR.get(itype, 0.10)

    # Pods by phase
    pods = _kubectl_json(context, "get", "pods", "--all-namespaces").get("items", [])
    for pod in pods:
        phase = pod.get("status", {}).get("phase", "")
        if phase == "Running":
            snap.running_pods += 1
        elif phase == "Pending":
            snap.pending_pods += 1

    # Karpenter disruption events
    try:
        events = _kubectl_json(context, "get", "events", "--all-namespaces").get("items", [])
        snap.disruption_events = sum(
            1 for e in events if e.get("reason") in _KARPENTER_REASONS
        )
    except (subprocess.CalledProcessError, json.JSONDecodeError):
        pass

    return snap


def scheduling_latencies(context: str) -> list[dict[str, Any]]:
    """Compute per-pod scheduling latency (seconds)."""
    pods = _kubectl_json(context, "get", "pods", "--all-namespaces").get("items", [])
    results = []
    for pod in pods:
        created = _parse_ts(pod.get("metadata", {}).get("creationTimestamp"))
        if not created:
            continue
        for cond in pod.get("status", {}).get("conditions", []):
            if cond.get("type") == "PodScheduled" and cond.get("status") == "True":
                scheduled = _parse_ts(cond.get("lastTransitionTime"))
                if scheduled > created:
                    results.append({
                        "pod": pod["metadata"].get("name", ""),
                        "namespace": pod["metadata"].get("namespace", ""),
                        "latency_s": scheduled - created,
                    })
                break
    return results


def collect_timeseries(
    context: str, duration: int, interval: int,
) -> list[MetricsSnapshot]:
    """Poll cluster at `interval` seconds for `duration` seconds."""
    snapshots: list[MetricsSnapshot] = []
    deadline = time.time() + duration
    while time.time() < deadline:
        try:
            snap = snapshot(context)
            snapshots.append(snap)
            print(
                f"[{datetime.fromtimestamp(snap.timestamp, tz=timezone.utc).strftime('%H:%M:%S')}] "
                f"nodes={snap.node_count} running={snap.running_pods} "
                f"pending={snap.pending_pods} $/hr={snap.cost_per_hour:.3f} "
                f"disruptions={snap.disruption_events}"
            )
        except (subprocess.CalledProcessError, json.JSONDecodeError) as exc:
            print(f"Warning: snapshot failed: {exc}", file=sys.stderr)
        remaining = interval - (time.time() - (snap.timestamp if snapshots else time.time()))
        if remaining > 0 and time.time() < deadline:
            time.sleep(min(remaining, deadline - time.time()))
    return snapshots


def export_parquet(snapshots: list[MetricsSnapshot], output: Path) -> None:
    """Export snapshots to parquet with the required schema."""
    rows = [
        {
            "timestamp": s.timestamp,
            "node_count": s.node_count,
            "running_pods": s.running_pods,
            "pending_pods": s.pending_pods,
            "cost_per_hour": s.cost_per_hour,
            "disruption_events": s.disruption_events,
        }
        for s in snapshots
    ]
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        import polars as pl
        pl.DataFrame(rows).write_parquet(output)
    except ImportError:
        try:
            import pyarrow as pa, pyarrow.parquet as pq
            pq.write_table(pa.Table.from_pylist(rows), str(output))
        except ImportError:
            fallback = output.with_suffix(".json")
            print(f"Warning: no parquet library available, writing {fallback}", file=sys.stderr)
            import json as _json
            fallback.write_text(_json.dumps(rows, indent=2))
            return
    print(f"Wrote {len(snapshots)} snapshots to {output}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="metrics_collector",
        description="Time-series metrics collector for KWOK/KIND clusters",
    )
    parser.add_argument("--context", required=True, help="kubectl context name")
    parser.add_argument("--duration", type=int, default=300,
                        help="Collection duration in seconds (default: 300)")
    parser.add_argument("--interval", type=int, default=30,
                        help="Polling interval in seconds (default: 30)")
    parser.add_argument("--output", "-o", type=Path, default=Path("metrics.parquet"),
                        help="Output parquet file (default: metrics.parquet)")
    args = parser.parse_args(argv)

    print(f"Collecting metrics from {args.context} every {args.interval}s for {args.duration}s")
    snapshots = collect_timeseries(args.context, args.duration, args.interval)

    if not snapshots:
        print("Error: no snapshots collected", file=sys.stderr)
        return 1

    export_parquet(snapshots, args.output)

    # Print scheduling latency summary
    lats = scheduling_latencies(args.context)
    if lats:
        avg = sum(l["latency_s"] for l in lats) / len(lats)
        print(f"Scheduling latency: {len(lats)} pods, avg={avg:.3f}s")

    return 0


if __name__ == "__main__":
    sys.exit(main())
