"""KWOK/KIND runner — spin up KIND cluster with KWOK fake nodes, apply manifests, collect metrics.

Exports metrics in the same schema as Tier 1 SimResult for cross-tier comparison.

Usage::

    kubesim validate-kwok manifests/ --output results.parquet
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Any


# ── SimResult-compatible output schema ───────────────────────────

@dataclass
class KwokResult:
    """Mirrors the Tier 1 SimResult schema for cross-tier comparison."""
    seed: int = 0
    variant: str = "kwok"
    events_processed: int = 0
    total_cost_per_hour: float = 0.0
    node_count: int = 0
    pod_count: int = 0
    running_pods: int = 0
    pending_pods: int = 0
    final_time: int = 0
    # Extended KWOK-specific fields
    scheduling_latencies_ns: list[int] = field(default_factory=list)
    node_count_over_time: list[dict[str, Any]] = field(default_factory=list)
    pod_phase_transitions: list[dict[str, Any]] = field(default_factory=list)

    def to_sim_result_dict(self) -> dict[str, Any]:
        """Return only the fields matching Tier 1 SimResult schema."""
        return {
            "seed": self.seed,
            "variant": self.variant,
            "events_processed": self.events_processed,
            "total_cost_per_hour": self.total_cost_per_hour,
            "node_count": self.node_count,
            "pod_count": self.pod_count,
            "running_pods": self.running_pods,
            "pending_pods": self.pending_pods,
            "final_time": self.final_time,
        }


# ── Shell helpers ────────────────────────────────────────────────

def _run(cmd: list[str], check: bool = True, capture: bool = True, **kw: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=check, capture_output=capture, text=True, **kw)


def _kubectl(args: list[str], cluster: str) -> subprocess.CompletedProcess[str]:
    return _run(["kubectl", "--context", f"kind-{cluster}"] + args)


def _check_tool(name: str) -> bool:
    try:
        _run(["which", name])
        return True
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False


# ── KIND cluster lifecycle ───────────────────────────────────────

def create_kind_cluster(name: str) -> None:
    """Create a minimal KIND cluster (control-plane only, KWOK provides fake nodes)."""
    import tempfile, yaml
    config = {
        "kind": "Cluster",
        "apiVersion": "kind.x-k8s.io/v1alpha4",
        "nodes": [{"role": "control-plane"}],
    }
    with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
        yaml.dump(config, f)
        cfg_path = f.name
    try:
        _run(["kind", "create", "cluster", "--name", name, "--config", cfg_path], capture=False)
    finally:
        Path(cfg_path).unlink(missing_ok=True)


def delete_kind_cluster(name: str) -> None:
    _run(["kind", "delete", "cluster", "--name", name], check=False, capture=False)


def install_kwok(cluster: str) -> None:
    """Install KWOK into the KIND cluster for fake node simulation."""
    kwok_release = "v0.7.0"
    base = f"https://github.com/kubernetes-sigs/kwok/releases/download/{kwok_release}"
    manifests = [
        f"{base}/kwok.yaml",
        f"{base}/stage-fast.yaml",
    ]
    for url in manifests:
        _kubectl(["apply", "-f", url], cluster)
    # Wait for KWOK controller to be ready
    _kubectl(["wait", "--for=condition=Available", "deployment/kwok-controller",
              "-n", "kube-system", "--timeout=120s"], cluster)


def create_kwok_nodes(cluster: str, count: int = 10) -> None:
    """Create fake KWOK nodes."""
    for i in range(count):
        node_manifest = {
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": {
                "name": f"kwok-node-{i}",
                "annotations": {"node.alpha.kubernetes.io/ttl": "0",
                                 "kwok.x-k8s.io/node": "fake"},
                "labels": {
                    "type": "kwok",
                    "node.kubernetes.io/instance-type": "m5.xlarge",
                    "topology.kubernetes.io/zone": f"us-east-1{'abc'[i % 3]}",
                },
            },
            "spec": {"taints": []},
            "status": {
                "allocatable": {
                    "cpu": "4", "memory": "16Gi", "pods": "110",
                },
                "capacity": {
                    "cpu": "4", "memory": "16Gi", "pods": "110",
                },
                "conditions": [
                    {"type": "Ready", "status": "True",
                     "reason": "KubeletReady", "message": "kwok fake node"},
                ],
            },
        }
        _run(["kubectl", "--context", f"kind-{cluster}", "apply", "-f", "-"],
             input=json.dumps(node_manifest), capture=False)


# ── Metrics collection ───────────────────────────────────────────

def _get_pods_json(cluster: str) -> list[dict[str, Any]]:
    r = _kubectl(["get", "pods", "--all-namespaces", "-o", "json"], cluster)
    data = json.loads(r.stdout)
    return data.get("items", [])


def _get_nodes_json(cluster: str) -> list[dict[str, Any]]:
    r = _kubectl(["get", "nodes", "-o", "json"], cluster)
    data = json.loads(r.stdout)
    return data.get("items", [])


def _parse_timestamp_ns(ts: str | None) -> int:
    """Parse K8s RFC3339 timestamp to nanoseconds since epoch."""
    if not ts:
        return 0
    from datetime import datetime, timezone
    # Handle fractional seconds
    ts = ts.rstrip("Z")
    for fmt in ("%Y-%m-%dT%H:%M:%S.%f", "%Y-%m-%dT%H:%M:%S"):
        try:
            dt = datetime.strptime(ts, fmt).replace(tzinfo=timezone.utc)
            return int(dt.timestamp() * 1_000_000_000)
        except ValueError:
            continue
    return 0


def collect_metrics(cluster: str) -> KwokResult:
    """Collect metrics from the KWOK/KIND cluster."""
    result = KwokResult()

    # Node metrics
    nodes = _get_nodes_json(cluster)
    result.node_count = len(nodes)
    result.node_count_over_time.append({
        "time_ns": int(time.time() * 1e9),
        "count": result.node_count,
    })

    # Pod metrics
    pods = _get_pods_json(cluster)
    result.pod_count = len(pods)

    for pod in pods:
        status = pod.get("status", {})
        phase = status.get("phase", "Unknown")
        name = pod.get("metadata", {}).get("name", "")

        if phase == "Running":
            result.running_pods += 1
        elif phase == "Pending":
            result.pending_pods += 1

        # Scheduling latency: time from creation to scheduled condition
        created = _parse_timestamp_ns(pod.get("metadata", {}).get("creationTimestamp"))
        scheduled_time = 0
        for cond in status.get("conditions", []):
            if cond.get("type") == "PodScheduled" and cond.get("status") == "True":
                scheduled_time = _parse_timestamp_ns(cond.get("lastTransitionTime"))
                break

        if created and scheduled_time and scheduled_time > created:
            result.scheduling_latencies_ns.append(scheduled_time - created)

        # Phase transitions
        for cond in status.get("conditions", []):
            result.pod_phase_transitions.append({
                "pod": name,
                "type": cond.get("type"),
                "status": cond.get("status"),
                "time": cond.get("lastTransitionTime"),
            })

    result.events_processed = len(pods) + len(nodes)
    if pods:
        times = [_parse_timestamp_ns(p.get("metadata", {}).get("creationTimestamp")) for p in pods]
        times = [t for t in times if t > 0]
        if times:
            result.final_time = max(times) - min(times)

    return result


# ── Export ────────────────────────────────────────────────────────

def export_results(result: KwokResult, output: Path) -> None:
    """Export KwokResult to parquet (or JSON fallback)."""
    sim_dict = result.to_sim_result_dict()

    if output.suffix == ".parquet":
        try:
            import pyarrow as pa
            import pyarrow.parquet as pq

            table = pa.Table.from_pylist([sim_dict])
            pq.write_table(table, str(output))
            return
        except ImportError:
            pass
        # Fallback: try polars
        try:
            import polars as pl

            df = pl.DataFrame([sim_dict])
            df.write_parquet(str(output))
            return
        except ImportError:
            # Last resort: write JSON with .parquet name (warn user)
            print("Warning: neither pyarrow nor polars available, writing JSON", file=sys.stderr)
            output = output.with_suffix(".json")

    # JSON output
    with open(output, "w") as f:
        json.dump(sim_dict, f, indent=2)
    print(f"Results written to {output}")


# ── Main runner ──────────────────────────────────────────────────

def run_kwok_validation(
    manifests_dir: Path,
    output: Path,
    variant: str = "kwok",
    node_count: int = 10,
    settle_seconds: int = 30,
    cleanup: bool = True,
) -> KwokResult:
    """Full KWOK/KIND validation run.

    1. Create KIND cluster
    2. Install KWOK
    3. Create fake nodes
    4. Apply manifests
    5. Wait for pods to settle
    6. Collect metrics
    7. Export results
    8. Cleanup
    """
    for tool in ("kind", "kubectl"):
        if not _check_tool(tool):
            print(f"Error: {tool} not found in PATH", file=sys.stderr)
            sys.exit(1)

    cluster_name = f"kubesim-{uuid.uuid4().hex[:8]}"
    print(f"Creating KIND cluster: {cluster_name}")

    try:
        create_kind_cluster(cluster_name)
        print("Installing KWOK...")
        install_kwok(cluster_name)
        print(f"Creating {node_count} fake KWOK nodes...")
        create_kwok_nodes(cluster_name, node_count)

        # Apply manifests
        manifest_files = sorted(manifests_dir.glob("*.yaml"))
        if not manifest_files:
            print(f"Error: no YAML files found in {manifests_dir}", file=sys.stderr)
            sys.exit(1)

        print(f"Applying {len(manifest_files)} manifests...")
        for mf in manifest_files:
            _kubectl(["apply", "-f", str(mf)], cluster_name)

        # Wait for pods to settle
        print(f"Waiting {settle_seconds}s for pods to settle...")
        time.sleep(settle_seconds)

        # Collect metrics
        print("Collecting metrics...")
        result = collect_metrics(cluster_name)
        result.variant = variant

        # Export
        export_results(result, output)
        print(f"Done. nodes={result.node_count} pods={result.pod_count} "
              f"running={result.running_pods} pending={result.pending_pods}")

        return result

    finally:
        if cleanup:
            print(f"Cleaning up cluster {cluster_name}...")
            delete_kind_cluster(cluster_name)
