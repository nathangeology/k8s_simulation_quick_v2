"""Translate KubeSim scenario YAML into real K8s manifests.

Generates Deployments, Jobs, HPAs, PDBs, Services, PriorityClasses,
KWOK Node templates, and Karpenter NodePool CRDs from a KubeSim study
definition. Output is a directory of YAML files ready for
``kubectl apply -f manifests/``.

Usage::

    kubesim translate scenario.yaml --output manifests/
"""

from __future__ import annotations

import json
import os
import math
from pathlib import Path
from typing import Any

import yaml

# ── KWOK catalog (loaded once) ───────────────────────────────────

_KWOK_CATALOG_PATH = Path(__file__).resolve().parents[3] / "crates" / "kubesim-ec2" / "src" / "kwok_instance_types.json"

_KWOK_CATALOG: list[dict[str, Any]] | None = None


def _load_kwok_catalog() -> list[dict[str, Any]]:
    global _KWOK_CATALOG
    if _KWOK_CATALOG is None:
        with open(_KWOK_CATALOG_PATH) as f:
            _KWOK_CATALOG = json.load(f)
    return _KWOK_CATALOG


# Well-known EC2 instance type specs: (vCPU, memory_GiB, family)
# family: c=compute, s=standard(general), m=memory
_EC2_SPECS: dict[str, tuple[int, int, str]] = {
    "t3.medium": (2, 4, "s"), "t3.large": (2, 8, "s"), "t3.xlarge": (4, 16, "s"),
    "m5.large": (2, 8, "s"), "m5.xlarge": (4, 16, "s"), "m5.2xlarge": (8, 32, "s"),
    "m5.4xlarge": (16, 64, "s"), "m5.8xlarge": (32, 128, "s"),
    "m6i.large": (2, 8, "s"), "m6i.xlarge": (4, 16, "s"), "m6i.2xlarge": (8, 32, "s"),
    "c5.large": (2, 4, "c"), "c5.xlarge": (4, 8, "c"), "c5.2xlarge": (8, 16, "c"),
    "c5.4xlarge": (16, 32, "c"),
    "r5.large": (2, 16, "m"), "r5.xlarge": (4, 32, "m"), "r5.2xlarge": (8, 64, "m"),
}


def _ec2_to_kwok_resources(instance_type: str) -> dict[str, str]:
    """Map an EC2 instance type to KWOK catalog resources."""
    spec = _EC2_SPECS.get(instance_type)
    if spec:
        cpu, mem_gi, family = spec
        kwok_name = f"{family}-{cpu}x-amd64-linux"
        for entry in _load_kwok_catalog():
            if entry["name"] == kwok_name:
                return dict(entry["resources"])
    # Fallback: 4 cpu, 16Gi (m5.xlarge equivalent)
    return {"cpu": "4", "memory": "16Gi", "ephemeral-storage": "20Gi", "pods": "64"}

# ── Workload archetype defaults ──────────────────────────────────

_ARCHETYPE_DEFAULTS: dict[str, dict[str, Any]] = {
    "web_app": {
        "cpu_request": "250m", "memory_request": "256Mi",
        "cpu_limit": "500m", "memory_limit": "512Mi",
        "replicas": 2, "port": 8080,
    },
    "ml_training": {
        "cpu_request": "8", "memory_request": "32Gi",
        "cpu_limit": "8", "memory_limit": "32Gi",
        "replicas": 1, "port": None,
    },
    "batch_job": {
        "cpu_request": "1", "memory_request": "2Gi",
        "cpu_limit": "2", "memory_limit": "4Gi",
        "replicas": 1, "port": None,
    },
    "saas_microservice": {
        "cpu_request": "500m", "memory_request": "512Mi",
        "cpu_limit": "1", "memory_limit": "1Gi",
        "replicas": 3, "port": 8080,
    },
}

_FALLBACK = {
    "cpu_request": "500m", "memory_request": "512Mi",
    "cpu_limit": "1", "memory_limit": "1Gi",
    "replicas": 1, "port": 8080,
}


def _dist_mean_str(dist: dict | str | int | float, kind: str = "cpu") -> str:
    """Extract a representative value from a distribution spec."""
    if isinstance(dist, (int, float)):
        return str(dist) if kind != "cpu" else f"{int(dist * 1000)}m" if dist < 10 else str(int(dist))
    if isinstance(dist, str):
        return dist
    d = dist.get("dist", "")
    if d in ("normal", "lognormal", "exponential"):
        return str(dist.get("mean", "0"))
    if d == "uniform":
        return str(dist.get("min", "0"))
    if d == "choice":
        vals = dist.get("values", [])
        return str(vals[0]) if vals else "0"
    if d == "poisson":
        return str(int(dist.get("lambda", 1)))
    return "0"


def _resolve_count(val: dict | int | float) -> int:
    if isinstance(val, (int, float)):
        return max(1, int(val))
    if isinstance(val, dict):
        d = val.get("dist", "")
        if d == "uniform":
            lo = val.get("min", 1)
            hi = val.get("max", 1)
            return max(1, int((lo + hi) / 2))
        if d == "poisson":
            return max(1, int(val.get("lambda", 1)))
    return 1


def _resolve_replicas(workload: dict) -> int:
    r = workload.get("replicas")
    if r is None:
        arch = workload.get("type", "")
        return _ARCHETYPE_DEFAULTS.get(arch, _FALLBACK)["replicas"]
    if isinstance(r, (int, float)):
        return max(1, int(r))
    if isinstance(r, dict):
        if "fixed" in r:
            return max(1, int(r["fixed"]))
        return max(1, int(r.get("min", 1)))
    return 1


def _resolve_resource(workload: dict, field: str, arch_key: str) -> str:
    val = workload.get(field)
    if val is not None:
        return _dist_mean_str(val, kind="cpu" if "cpu" in field else "mem")
    arch = workload.get("type", "")
    return _ARCHETYPE_DEFAULTS.get(arch, _FALLBACK).get(arch_key, "500m")


def _make_labels(study_name: str, workload_name: str, idx: int) -> dict[str, str]:
    safe = workload_name.replace("_", "-")
    return {
        "app.kubernetes.io/name": f"{safe}-{idx}",
        "app.kubernetes.io/part-of": study_name,
        "kubesim.io/workload-type": workload_name,
    }


# ── Manifest generators ─────────────────────────────────────────

def _deployment(study_name: str, workload: dict, idx: int, variant: dict | None = None) -> dict:
    wtype = workload.get("type", "workload")
    safe = wtype.replace("_", "-")
    name = f"{safe}-{idx}"
    labels = _make_labels(study_name, wtype, idx)
    replicas = _resolve_replicas(workload)

    cpu_req = _resolve_resource(workload, "cpu_request", "cpu_request")
    mem_req = _resolve_resource(workload, "memory_request", "memory_request")
    cpu_lim = _resolve_resource(workload, "cpu_limit", "cpu_limit")
    mem_lim = _resolve_resource(workload, "memory_limit", "memory_limit")
    # If no explicit limit fields, use archetype limits
    if workload.get("cpu_limit") is None:
        arch = _ARCHETYPE_DEFAULTS.get(wtype, _FALLBACK)
        cpu_lim = arch["cpu_limit"]
    if workload.get("memory_limit") is None:
        arch = _ARCHETYPE_DEFAULTS.get(wtype, _FALLBACK)
        mem_lim = arch["memory_limit"]

    gpu = workload.get("gpu_request")
    resources: dict[str, Any] = {
        "requests": {"cpu": cpu_req, "memory": mem_req},
        "limits": {"cpu": cpu_lim, "memory": mem_lim},
    }
    if gpu is not None:
        gpu_val = _dist_mean_str(gpu, kind="gpu")
        resources["requests"]["nvidia.com/gpu"] = gpu_val
        resources["limits"]["nvidia.com/gpu"] = gpu_val

    priority = workload.get("priority")
    priority_class = _priority_class_name(priority) if priority and priority in _PRIORITY_CLASSES else None

    container: dict[str, Any] = {
        "name": safe,
        "image": f"kubesim/{safe}:latest",
        "resources": resources,
    }

    arch = _ARCHETYPE_DEFAULTS.get(wtype, _FALLBACK)
    if arch.get("port"):
        container["ports"] = [{"containerPort": arch["port"]}]

    pod_spec: dict[str, Any] = {"containers": [container]}
    if priority_class:
        pod_spec["priorityClassName"] = priority_class

    # Topology spread constraints
    tsc = workload.get("topology_spread")
    if tsc:
        pod_spec["topologySpreadConstraints"] = [{
            "maxSkew": tsc.get("max_skew", 1),
            "topologyKey": tsc.get("topology_key", "topology.kubernetes.io/zone"),
            "whenUnsatisfiable": "DoNotSchedule",
            "labelSelector": {"matchLabels": labels},
        }]

    # Deletion cost annotation from variant
    annotations = {}
    if variant and variant.get("deletion_cost_strategy") == "prefer_emptying_nodes":
        annotations["controller.kubernetes.io/pod-deletion-cost"] = "0"

    template_meta: dict[str, Any] = {"labels": labels}
    if annotations:
        template_meta["annotations"] = annotations

    return {
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {"name": name, "labels": labels},
        "spec": {
            "replicas": replicas,
            "selector": {"matchLabels": {"app.kubernetes.io/name": labels["app.kubernetes.io/name"]}},
            "template": {
                "metadata": template_meta,
                "spec": pod_spec,
            },
        },
    }


def _hpa(study_name: str, workload: dict, idx: int) -> dict | None:
    scaling = workload.get("scaling")
    if not scaling or scaling.get("type") == "none":
        return None
    if scaling.get("type") != "hpa":
        return None

    wtype = workload.get("type", "workload")
    safe = wtype.replace("_", "-")
    name = f"{safe}-{idx}"

    replicas = workload.get("replicas", {})
    min_r = replicas.get("min", 1) if isinstance(replicas, dict) else 1
    max_r = replicas.get("max", 10) if isinstance(replicas, dict) else 10

    metric = scaling.get("metric", "cpu")
    target = scaling.get("target", "70%")
    target_str = str(target)

    hpa: dict[str, Any] = {
        "apiVersion": "autoscaling/v2",
        "kind": "HorizontalPodAutoscaler",
        "metadata": {"name": f"{name}-hpa"},
        "spec": {
            "scaleTargetRef": {
                "apiVersion": "apps/v1",
                "kind": "Deployment",
                "name": name,
            },
            "minReplicas": max(1, int(min_r)),
            "maxReplicas": max(1, int(max_r)),
            "metrics": [],
        },
    }

    if metric in ("cpu", "memory"):
        target_val = int(target_str.rstrip("%")) if target_str.endswith("%") else int(float(target_str))
        hpa["spec"]["metrics"].append({
            "type": "Resource",
            "resource": {
                "name": metric,
                "target": {"type": "Utilization", "averageUtilization": target_val},
            },
        })
    else:
        # Custom metric (e.g. rps)
        target_val = int(float(target_str.rstrip("%"))) if target_str.endswith("%") else int(float(target_str))
        hpa["spec"]["metrics"].append({
            "type": "Pods",
            "pods": {
                "metric": {"name": metric},
                "target": {"type": "AverageValue", "averageValue": str(target_val)},
            },
        })

    return hpa


def _pdb(study_name: str, workload: dict, idx: int, variant: dict | None = None) -> dict | None:
    pdb_spec = workload.get("pdb")
    if not pdb_spec and variant:
        pdb_spec = variant.get("pdb")
    if not pdb_spec:
        return None

    wtype = workload.get("type", "workload")
    safe = wtype.replace("_", "-")
    name = f"{safe}-{idx}"
    labels = _make_labels(study_name, wtype, idx)

    spec: dict[str, Any] = {
        "selector": {"matchLabels": {"app.kubernetes.io/name": labels["app.kubernetes.io/name"]}},
    }
    if "min_available" in pdb_spec:
        spec["minAvailable"] = pdb_spec["min_available"]
    elif "max_unavailable" in pdb_spec:
        spec["maxUnavailable"] = pdb_spec["max_unavailable"]

    return {
        "apiVersion": "policy/v1",
        "kind": "PodDisruptionBudget",
        "metadata": {"name": f"{name}-pdb"},
        "spec": spec,
    }


def _service(study_name: str, workload: dict, idx: int) -> dict | None:
    wtype = workload.get("type", "workload")
    arch = _ARCHETYPE_DEFAULTS.get(wtype, _FALLBACK)
    port = arch.get("port")
    if not port:
        return None

    safe = wtype.replace("_", "-")
    name = f"{safe}-{idx}"
    labels = _make_labels(study_name, wtype, idx)

    return {
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {"name": f"{name}-svc"},
        "spec": {
            "selector": {"app.kubernetes.io/name": labels["app.kubernetes.io/name"]},
            "ports": [{"port": port, "targetPort": port, "protocol": "TCP"}],
        },
    }


def _nodepool_crd(pool: dict, pool_idx: int) -> dict:
    """Generate a Karpenter NodePool CRD from scenario node pool config."""
    name = f"kubesim-pool-{pool_idx}"
    instance_types = pool.get("instance_types", [])
    max_nodes = pool.get("max_nodes", 100)

    requirements = []
    if instance_types:
        requirements.append({
            "key": "node.kubernetes.io/instance-type",
            "operator": "In",
            "values": instance_types,
        })
    requirements.append({
        "key": "kubernetes.io/arch",
        "operator": "In",
        "values": ["amd64"],
    })
    requirements.append({
        "key": "karpenter.sh/capacity-type",
        "operator": "In",
        "values": ["on-demand"],
    })

    spec: dict[str, Any] = {
        "template": {
            "spec": {
                "requirements": requirements,
                "nodeClassRef": {
                    "apiVersion": "karpenter.k8s.aws/v1",
                    "kind": "EC2NodeClass",
                    "name": name,
                },
            },
        },
        "limits": {"cpu": str(max_nodes * 4)},  # rough estimate
    }

    karpenter = pool.get("karpenter", {}) or {}
    consolidation = karpenter.get("consolidation")
    if consolidation:
        policy = consolidation.get("policy", "WhenUnderutilized")
        spec["disruption"] = {
            "consolidationPolicy": policy,
            "consolidateAfter": "30s",
        }

    return {
        "apiVersion": "karpenter.sh/v1",
        "kind": "NodePool",
        "metadata": {"name": name},
        "spec": spec,
    }


def _ec2nodeclass(pool_idx: int) -> dict:
    """Generate a minimal EC2NodeClass companion for the NodePool."""
    name = f"kubesim-pool-{pool_idx}"
    return {
        "apiVersion": "karpenter.k8s.aws/v1",
        "kind": "EC2NodeClass",
        "metadata": {"name": name},
        "spec": {
            "amiSelectorTerms": [{"alias": "al2023@latest"}],
            "subnetSelectorTerms": [{"tags": {"karpenter.sh/discovery": "kubesim"}}],
            "securityGroupSelectorTerms": [{"tags": {"karpenter.sh/discovery": "kubesim"}}],
        },
    }


def _kwok_node(instance_type: str, node_idx: int, pool_idx: int) -> dict:
    """Generate a KWOK fake Node manifest with allocatable resources from the catalog."""
    resources = _ec2_to_kwok_resources(instance_type)
    return {
        "apiVersion": "v1",
        "kind": "Node",
        "metadata": {
            "name": f"kwok-pool{pool_idx}-node-{node_idx}",
            "annotations": {
                "node.alpha.kubernetes.io/ttl": "0",
                "kwok.x-k8s.io/node": "fake",
            },
            "labels": {
                "type": "kwok",
                "node.kubernetes.io/instance-type": instance_type,
                "topology.kubernetes.io/zone": f"us-east-1{'abc'[node_idx % 3]}",
            },
        },
        "spec": {"taints": []},
        "status": {
            "allocatable": resources,
            "capacity": resources,
            "conditions": [
                {"type": "Ready", "status": "True",
                 "reason": "KubeletReady", "message": "kwok fake node"},
            ],
        },
    }


def _job(study_name: str, workload: dict, idx: int, variant: dict | None = None) -> dict:
    """Generate a Job manifest for batch_job workloads."""
    wtype = workload.get("type", "workload")
    safe = wtype.replace("_", "-")
    name = f"{safe}-{idx}"
    labels = _make_labels(study_name, wtype, idx)

    cpu_req = _resolve_resource(workload, "cpu_request", "cpu_request")
    mem_req = _resolve_resource(workload, "memory_request", "memory_request")
    cpu_lim = _resolve_resource(workload, "cpu_limit", "cpu_limit")
    mem_lim = _resolve_resource(workload, "memory_limit", "memory_limit")
    if workload.get("cpu_limit") is None:
        cpu_lim = _ARCHETYPE_DEFAULTS.get(wtype, _FALLBACK)["cpu_limit"]
    if workload.get("memory_limit") is None:
        mem_lim = _ARCHETYPE_DEFAULTS.get(wtype, _FALLBACK)["memory_limit"]

    resources: dict[str, Any] = {
        "requests": {"cpu": cpu_req, "memory": mem_req},
        "limits": {"cpu": cpu_lim, "memory": mem_lim},
    }

    priority = workload.get("priority")
    priority_class = _priority_class_name(priority) if priority else None

    pod_spec: dict[str, Any] = {
        "containers": [{
            "name": safe,
            "image": f"kubesim/{safe}:latest",
            "resources": resources,
        }],
        "restartPolicy": "Never",
    }
    if priority_class:
        pod_spec["priorityClassName"] = priority_class

    return {
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {"name": name, "labels": labels},
        "spec": {
            "completions": 1,
            "parallelism": 1,
            "backoffLimit": 3,
            "template": {
                "metadata": {"labels": labels},
                "spec": pod_spec,
            },
        },
    }


_PRIORITY_CLASSES: dict[str, int] = {
    "low": 100,
    "medium": 1000,
    "high": 10000,
    "critical": 100000,
}


def _priority_class_name(priority: str) -> str:
    return f"{priority}-priority"


def _priority_class_manifests() -> list[dict]:
    """Generate PriorityClass definitions for all standard levels."""
    return [
        {
            "apiVersion": "scheduling.k8s.io/v1",
            "kind": "PriorityClass",
            "metadata": {"name": _priority_class_name(name)},
            "value": value,
            "globalDefault": False,
            "description": f"KubeSim {name} priority class",
        }
        for name, value in _PRIORITY_CLASSES.items()
    ]


def _namespace(study_name: str) -> dict:
    return {
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {"name": study_name},
    }


# ── Public API ───────────────────────────────────────────────────

def translate_scenario(
    scenario_path: str | Path,
    output_dir: str | Path,
    variant_name: str | None = None,
) -> list[Path]:
    """Translate a KubeSim scenario YAML into K8s manifests.

    Args:
        scenario_path: Path to the scenario YAML file.
        output_dir: Directory to write manifest YAML files.
        variant_name: If set, apply variant-specific config (PDBs, deletion cost).

    Returns:
        List of paths to generated manifest files.
    """
    scenario_path = Path(scenario_path)
    output_dir = Path(output_dir)

    with open(scenario_path) as f:
        raw = yaml.safe_load(f)

    study = raw.get("study", raw)
    study_name = study.get("name", "kubesim")

    # Find variant if specified
    variant = None
    if variant_name:
        for v in study.get("variants", []):
            if v.get("name") == variant_name:
                variant = v
                break

    manifests: list[dict] = []
    written: list[Path] = []

    # Namespace
    manifests.append(_namespace(study_name))

    # PriorityClasses (emitted before workloads that reference them)
    manifests.extend(_priority_class_manifests())

    # Node pools → Karpenter NodePool CRDs + EC2NodeClass + KWOK Node templates
    for i, pool in enumerate(study.get("cluster", {}).get("node_pools", [])):
        manifests.append(_nodepool_crd(pool, i))
        manifests.append(_ec2nodeclass(i))
        # KWOK fake nodes for min_nodes
        min_nodes = pool.get("min_nodes", 0)
        instance_types = pool.get("instance_types", [])
        for n in range(min_nodes):
            itype = instance_types[n % len(instance_types)] if instance_types else "m5.xlarge"
            manifests.append(_kwok_node(itype, n, i))

    # Workloads → Deployments/Jobs, HPAs, PDBs, Services
    owner_idx = 0
    for workload in study.get("workloads", []):
        count = _resolve_count(workload.get("count", 1))
        wtype = workload.get("type", "")
        for c in range(count):
            idx = owner_idx
            owner_idx += 1

            if wtype == "batch_job":
                manifests.append(_job(study_name, workload, idx, variant))
            else:
                manifests.append(_deployment(study_name, workload, idx, variant))

            hpa = _hpa(study_name, workload, idx)
            if hpa:
                manifests.append(hpa)

            pdb = _pdb(study_name, workload, idx, variant)
            if pdb:
                manifests.append(pdb)

            svc = _service(study_name, workload, idx)
            if svc:
                manifests.append(svc)

    # Write manifests
    output_dir.mkdir(parents=True, exist_ok=True)
    for manifest in manifests:
        kind = manifest["kind"].lower()
        name = manifest["metadata"]["name"]
        filename = f"{kind}-{name}.yaml"
        path = output_dir / filename
        with open(path, "w") as f:
            yaml.dump(manifest, f, default_flow_style=False, sort_keys=False)
        written.append(path)

    return written
