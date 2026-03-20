"""Intermediate representation and abstract adapter for scenario conversion."""

from __future__ import annotations

import abc
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path


@dataclass
class NodePool:
    instance_types: list[str]
    min_nodes: int = 0
    max_nodes: int = 1000
    consolidation_policy: str = "WhenUnderutilized"


@dataclass
class DaemonSet:
    name: str
    cpu_request: str
    memory_request: str


@dataclass
class Delays:
    node_startup: str = "30s"
    node_startup_jitter: str = "10s"
    node_shutdown: str = "5s"
    provisioner_batch: str = "10s"
    provisioner_batch_jitter: str = "5s"
    pod_startup: str = "2s"


@dataclass
class Cluster:
    node_pools: list[NodePool] = field(default_factory=list)
    daemonsets: list[DaemonSet] = field(default_factory=list)
    delays: Delays = field(default_factory=Delays)


@dataclass
class Workload:
    name: str
    workload_type: str = "web_app"
    initial_replicas: int = 1
    cpu_request: str = "0m"
    memory_request: str = "0Mi"
    labels: dict[str, str] = field(default_factory=dict)
    # Affinity
    pod_anti_affinity: dict | None = None  # {label_key, topology_key, affinity_type}
    topology_spread: dict | None = None  # {max_skew, topology_key}
    # Scaling timeline: list of (time_minutes, absolute_replicas)
    scaling_timeline: list[tuple[int, int]] = field(default_factory=list)


@dataclass
class Variant:
    name: str
    scheduler: dict = field(default_factory=dict)


@dataclass
class ConversionMetadata:
    source_format: str
    source_path: str
    converted_at: str = field(
        default_factory=lambda: datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    )


@dataclass
class ScenarioIR:
    """Intermediate representation for a complete scenario."""

    name: str
    cluster: Cluster
    workloads: list[Workload]
    variants: list[Variant] = field(default_factory=list)
    metadata: ConversionMetadata = field(
        default_factory=lambda: ConversionMetadata("unknown", "unknown")
    )
    runs: int = 50
    time_mode: str = "wall_clock"


class FormatAdapter(abc.ABC):
    """Abstract base for input format adapters."""

    @abc.abstractmethod
    def name(self) -> str:
        """Short identifier for this format (e.g. 'k8s-import')."""

    @abc.abstractmethod
    def convert(self, input_path: Path) -> ScenarioIR:
        """Convert input at *input_path* to the intermediate representation."""
