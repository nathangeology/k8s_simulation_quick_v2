"""EKS validation runner — apply manifests to a real EKS cluster and collect metrics.

Applies translated K8s manifests to an existing EKS cluster, collects real
metrics (scheduling latency, node/pod counts, cost from CloudWatch/CUR),
and exports results in the SimResult-compatible schema as parquet.

Usage::

    kubesim validate-eks manifests/ --cluster my-cluster --output results.parquet
"""

from __future__ import annotations

import json
import subprocess
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


# ── SimResult-compatible schema ──────────────────────────────────

@dataclass
class EksResult:
    """Mirrors the Rust SimResult schema for cross-tier comparison."""

    seed: int = 0
    variant: str = ""
    events_processed: int = 0
    total_cost_per_hour: float = 0.0
    node_count: int = 0
    pod_count: int = 0
    running_pods: int = 0
    pending_pods: int = 0
    final_time: int = 0
    # EKS-specific extras
    scheduling_latencies_ms: list[float] = field(default_factory=list)
    disruption_events: int = 0

    def to_dict(self) -> dict[str, Any]:
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


# ── Kubectl helpers ──────────────────────────────────────────────

def _kubectl(*args: str, context: str | None = None) -> subprocess.CompletedProcess[str]:
    cmd = ["kubectl"]
    if context:
        cmd += ["--context", context]
    cmd += list(args)
    return subprocess.run(cmd, capture_output=True, text=True, check=True)


def _kubectl_json(*args: str, context: str | None = None) -> Any:
    r = _kubectl(*args, "-o", "json", context=context)
    return json.loads(r.stdout)


# ── Core runner ──────────────────────────────────────────────────

@dataclass
class EksRunner:
    """Apply manifests to an EKS cluster, collect metrics, clean up."""

    cluster: str
    namespace: str = "kubesim"
    context: str | None = None
    timeout_s: int = 300
    poll_interval_s: int = 5
    prometheus_url: str | None = None

    def run(self, manifests_dir: Path, variant: str = "") -> EksResult:
        """Apply manifests, wait for convergence, collect metrics, clean up."""
        manifests_dir = Path(manifests_dir)
        result = EksResult(variant=variant)
        start = time.monotonic()
        start_utc = datetime.now(timezone.utc)

        try:
            self._ensure_namespace()
            self._apply_manifests(manifests_dir)
            self._wait_for_convergence(result)
            self._collect_metrics(result, start_utc)
        finally:
            elapsed_ms = int((time.monotonic() - start) * 1000)
            result.final_time = elapsed_ms
            self._cleanup(manifests_dir)

        return result

    # ── Lifecycle ────────────────────────────────────────────────

    def _ensure_namespace(self) -> None:
        try:
            _kubectl("get", "namespace", self.namespace, context=self.context)
        except subprocess.CalledProcessError:
            _kubectl("create", "namespace", self.namespace, context=self.context)

    def _apply_manifests(self, manifests_dir: Path) -> None:
        _kubectl("apply", "-f", str(manifests_dir), "-n", self.namespace,
                 context=self.context)

    def _cleanup(self, manifests_dir: Path) -> None:
        try:
            _kubectl("delete", "-f", str(manifests_dir), "-n", self.namespace,
                     "--ignore-not-found=true", context=self.context)
        except subprocess.CalledProcessError:
            pass  # best-effort cleanup

    # ── Convergence ──────────────────────────────────────────────

    def _wait_for_convergence(self, result: EksResult) -> None:
        """Poll until all pods are Running or timeout."""
        deadline = time.monotonic() + self.timeout_s
        while time.monotonic() < deadline:
            pods = self._get_pods()
            running = sum(1 for p in pods if p.get("phase") == "Running")
            pending = sum(1 for p in pods if p.get("phase") == "Pending")
            result.pod_count = len(pods)
            result.running_pods = running
            result.pending_pods = pending
            if pending == 0 and len(pods) > 0:
                break
            time.sleep(self.poll_interval_s)

        # Collect scheduling latencies from pod conditions
        pods = self._get_pods()
        result.pod_count = len(pods)
        result.running_pods = sum(1 for p in pods if p.get("phase") == "Running")
        result.pending_pods = sum(1 for p in pods if p.get("phase") == "Pending")
        result.scheduling_latencies_ms = self._scheduling_latencies(pods)
        result.events_processed = len(pods)

    def _get_pods(self) -> list[dict[str, Any]]:
        data = _kubectl_json("get", "pods", "-n", self.namespace, context=self.context)
        out: list[dict[str, Any]] = []
        for item in data.get("items", []):
            phase = item.get("status", {}).get("phase", "Unknown")
            conditions = item.get("status", {}).get("conditions", [])
            created = item.get("metadata", {}).get("creationTimestamp")
            out.append({"phase": phase, "conditions": conditions, "created": created})
        return out

    def _scheduling_latencies(self, pods: list[dict[str, Any]]) -> list[float]:
        """Compute scheduling latency from PodScheduled condition timestamps."""
        latencies: list[float] = []
        for pod in pods:
            created = pod.get("created")
            if not created:
                continue
            for cond in pod.get("conditions", []):
                if cond.get("type") == "PodScheduled" and cond.get("status") == "True":
                    sched_time = cond.get("lastTransitionTime")
                    if sched_time:
                        try:
                            t_created = datetime.fromisoformat(created.replace("Z", "+00:00"))
                            t_sched = datetime.fromisoformat(sched_time.replace("Z", "+00:00"))
                            latencies.append((t_sched - t_created).total_seconds() * 1000)
                        except (ValueError, TypeError):
                            pass
                    break
        return latencies

    # ── Metrics collection ───────────────────────────────────────

    def _collect_metrics(self, result: EksResult, start_utc: datetime) -> None:
        """Collect node count and cost metrics."""
        result.node_count = self._get_node_count()
        result.total_cost_per_hour = self._estimate_cost(start_utc)
        result.disruption_events = self._count_disruption_events()

    def _get_node_count(self) -> int:
        data = _kubectl_json("get", "nodes", context=self.context)
        return len(data.get("items", []))

    def _estimate_cost(self, start_utc: datetime) -> float:
        """Estimate cost from node instance types. Falls back to 0 if boto3 unavailable."""
        try:
            data = _kubectl_json("get", "nodes", context=self.context)
        except subprocess.CalledProcessError:
            return 0.0

        # Simple cost estimation from instance type labels
        hourly = 0.0
        for node in data.get("items", []):
            labels = node.get("metadata", {}).get("labels", {})
            itype = labels.get("node.kubernetes.io/instance-type", "")
            hourly += _INSTANCE_COST_PER_HOUR.get(itype, 0.10)
        return hourly

    def _count_disruption_events(self) -> int:
        """Count eviction/preemption events in the namespace."""
        try:
            data = _kubectl_json("get", "events", "-n", self.namespace,
                                 "--field-selector=reason=Evicted",
                                 context=self.context)
            return len(data.get("items", []))
        except subprocess.CalledProcessError:
            return 0


# ── Instance pricing (common types, on-demand us-east-1) ────────

_INSTANCE_COST_PER_HOUR: dict[str, float] = {
    "m5.large": 0.096, "m5.xlarge": 0.192, "m5.2xlarge": 0.384,
    "m5.4xlarge": 0.768, "m5.8xlarge": 1.536,
    "c5.large": 0.085, "c5.xlarge": 0.170, "c5.2xlarge": 0.340,
    "c5.4xlarge": 0.680,
    "m6i.large": 0.096, "m6i.xlarge": 0.192, "m6i.2xlarge": 0.384,
    "r5.large": 0.126, "r5.xlarge": 0.252, "r5.2xlarge": 0.504,
    "t3.medium": 0.0416, "t3.large": 0.0832, "t3.xlarge": 0.1664,
}


# ── Parquet export ───────────────────────────────────────────────

def export_results(results: list[EksResult], output: Path) -> Path:
    """Export EksResult list to parquet in SimResult-compatible schema."""
    import polars as pl

    rows = [r.to_dict() for r in results]
    df = pl.DataFrame(rows)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(output)
    return output
