"""Adapter for the Grafana-style k8s import format.

Expects a directory containing:
  - config.yml   — cluster config (node type, pool size, autoscaling, deployment list)
  - steps.yml    — scaling timeline (may be 13K+ lines; streamed line-by-line)
  - deployments/ — one YAML per deployment with resource requests, affinity, topology spread
"""

from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

import yaml

from .base import (
    Cluster,
    ConversionMetadata,
    DaemonSet,
    Delays,
    FormatAdapter,
    NodePool,
    ScenarioIR,
    Workload,
)

# EC2 m5 family sizes used for bin-packing
_M5_FAMILY = [
    "m5.large", "m5.xlarge", "m5.2xlarge", "m5.4xlarge",
    "m5.8xlarge", "m5.12xlarge", "m5.16xlarge", "m5.24xlarge",
]


def _normalize_cpu(val: str) -> str:
    """Normalize CPU value to millicores string."""
    val = str(val).strip()
    if val.endswith("m"):
        return val
    return f"{int(float(val) * 1000)}m"


def _normalize_memory(val: str) -> str:
    """Normalize memory value to Mi string."""
    val = str(val).strip()
    if val.endswith("Mi"):
        return val
    if val.endswith("Gi"):
        return f"{int(float(val[:-2]) * 1024)}Mi"
    return val


def _parse_steps_streaming(path: Path) -> dict[str, list[tuple[int, int]]]:
    """Parse steps.yml line-by-line (handles 13K+ lines without loading all into memory)."""
    timelines: dict[str, list[tuple[int, int]]] = defaultdict(list)
    current_step: int | None = None
    for line in path.open():
        m = re.match(r"\s+name:\s+(\d+)", line)
        if m:
            current_step = int(m.group(1))
            continue
        m = re.match(r"\s+action_data:\s+name=([^,]+),replicas=(\d+)", line)
        if m and current_step is not None:
            timelines[m.group(1)].append((current_step, int(m.group(2))))
    return dict(timelines)


def _parse_deployment(path: Path) -> dict:
    """Parse a single deployment YAML, returning structured info."""
    with path.open() as f:
        doc = yaml.safe_load(f)
    name = doc["metadata"]["name"]
    spec = doc["spec"]
    pod_spec = spec["template"]["spec"]
    container = pod_spec["containers"][0]
    req = container.get("resources", {}).get("requests", {})

    result: dict = {
        "name": name,
        "replicas": spec.get("replicas", 1),
        "cpu": req.get("cpu", "0"),
        "memory": req.get("memory", "0"),
        "labels": spec["template"]["metadata"].get("labels", {}),
    }

    # Parse affinity
    affinity = pod_spec.get("affinity", {})
    anti = affinity.get("podAntiAffinity", {})
    required = anti.get("requiredDuringSchedulingIgnoredDuringExecution", [])
    if required:
        rule = required[0]
        # Extract the label key from matchExpressions
        label_key = "app"
        for expr in rule.get("labelSelector", {}).get("matchExpressions", []):
            if expr.get("operator") == "In":
                label_key = expr["key"]
                break
        result["pod_anti_affinity"] = {
            "label_key": label_key,
            "topology_key": rule.get("topologyKey", "kubernetes.io/hostname"),
            "affinity_type": "required",
        }

    # Parse topology spread constraints
    tsc = pod_spec.get("topologySpreadConstraints", [])
    if tsc:
        constraint = tsc[0]
        result["topology_spread"] = {
            "max_skew": constraint.get("maxSkew", 1),
            "topology_key": constraint.get("topologyKey", "kubernetes.io/hostname"),
        }

    return result


class K8sImportAdapter(FormatAdapter):
    """Adapter for the Grafana-style k8s import format."""

    def name(self) -> str:
        return "k8s-import"

    def convert(self, input_path: Path) -> ScenarioIR:
        input_path = Path(input_path)
        config_file = input_path / "config.yml"
        steps_file = input_path / "steps.yml"
        deploy_dir = input_path / "deployments"

        # Parse config
        with config_file.open() as f:
            config = yaml.safe_load(f)

        sim = config["simulator"]
        cluster_cfg = sim["clusters"][0]["KubernetesCluster"]
        node_type = cluster_cfg.get("node_type", "m5.large")

        # Determine instance type family
        base = node_type.split(".")[0] if "." in node_type else "m5"
        if base == "m5":
            instance_types = _M5_FAMILY
        else:
            instance_types = [node_type]

        # Parse deployments
        deployments: dict[str, dict] = {}
        for f in sorted(deploy_dir.glob("*.yaml")):
            if f.name.endswith("-pdb.yaml"):
                continue
            info = _parse_deployment(f)
            deployments[info["name"]] = info

        # Parse scaling timeline (streaming)
        timelines = _parse_steps_streaming(steps_file)

        # Build IR
        node_pool = NodePool(
            instance_types=instance_types,
            min_nodes=0,
            max_nodes=cluster_cfg.get("instance_pool_size", sim.get("instance_pool_size", 1000)),
        )

        cluster = Cluster(
            node_pools=[node_pool],
            daemonsets=[
                DaemonSet("kube-proxy", "100m", "256Mi"),
                DaemonSet("node-agent", "50m", "256Mi"),
            ],
            delays=Delays(),
        )

        workloads: list[Workload] = []
        for name in sorted(deployments.keys()):
            dep = deployments[name]
            w = Workload(
                name=name,
                initial_replicas=dep["replicas"],
                cpu_request=_normalize_cpu(dep["cpu"]),
                memory_request=_normalize_memory(dep["memory"]),
                labels={"app": name},
                pod_anti_affinity=dep.get("pod_anti_affinity"),
                topology_spread=dep.get("topology_spread"),
                scaling_timeline=sorted(timelines.get(name, [])),
            )
            workloads.append(w)

        num_deploys = len(deployments)
        scenario_name = input_path.name

        return ScenarioIR(
            name=scenario_name,
            cluster=cluster,
            workloads=workloads,
            metadata=ConversionMetadata(
                source_format=self.name(),
                source_path=str(input_path),
            ),
            runs=sim.get("runs", 50),
        )
