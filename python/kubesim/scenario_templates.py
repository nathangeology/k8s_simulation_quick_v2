"""Pre-built parameterized scenario templates for common trouble patterns.

Each template returns a valid scenario dict that can be fed to ``batch_run``
or the adversarial finder.

Usage::

    from kubesim.scenario_templates import bin_packing_stress, spot_interruption_storm

    scenario = bin_packing_stress("m5.large", "1900m", 50)
    scenario = spot_interruption_storm(0.8, 0.3)
"""

from __future__ import annotations


def _base_scenario(name: str, pools: list[dict], workloads: list[dict], **extra) -> dict:
    """Build a minimal valid scenario dict."""
    study = {
        "name": name,
        "runs": 1000,
        "time_mode": "logical",
        "cluster": {"node_pools": pools},
        "workloads": workloads,
        "variants": [],
        "metrics": {"compare": []},
    }
    study.update(extra)
    return {"study": study}


def bin_packing_stress(instance_type: str = "m5.large", pod_cpu: str = "1900m", pod_count: int = 50) -> dict:
    """Exact-fit scenarios where pods barely fit (or barely don't) on nodes."""
    return _base_scenario(
        "bin-packing-stress",
        pools=[{"instance_types": [instance_type], "min_nodes": 1, "max_nodes": pod_count}],
        workloads=[{
            "type": "web_app", "count": 1,
            "replicas": {"min": pod_count, "max": pod_count},
            "churn": "low", "traffic": "steady",
            "cpu_request": {"dist": "uniform", "min": pod_cpu, "max": pod_cpu},
        }],
    )


def consolidation_cascade(initial_nodes: int = 20, scale_down_pct: float = 0.8) -> dict:
    """Chain-reaction drains when nodes consolidate after scale-down."""
    remaining = max(1, int(initial_nodes * (1 - scale_down_pct)))
    return _base_scenario(
        "consolidation-cascade",
        pools=[{
            "instance_types": ["m5.xlarge"],
            "min_nodes": remaining, "max_nodes": initial_nodes,
            "karpenter": {"consolidation": {"policy": "WhenUnderutilized"}},
        }],
        workloads=[
            {"type": "web_app", "count": 5, "replicas": {"min": 3, "max": initial_nodes * 2},
             "churn": "high", "traffic": "diurnal",
             "pdb": {"min_available": "50%"}},
            {"type": "batch_job", "count": 10, "priority": "low",
             "duration": {"dist": "exponential", "mean": "10m"}},
        ],
    )


def spot_interruption_storm(spot_pct: float = 0.8, interrupt_rate: float = 0.3) -> dict:
    """Mass spot reclaim — high spot fraction with interruption pressure."""
    spot_nodes = max(1, int(100 * spot_pct))
    od_nodes = max(1, 100 - spot_nodes)
    return _base_scenario(
        "spot-interruption-storm",
        pools=[
            {"instance_types": ["m5.xlarge", "m5.2xlarge"], "min_nodes": 1, "max_nodes": spot_nodes},
            {"instance_types": ["m5.xlarge"], "min_nodes": 1, "max_nodes": od_nodes},
        ],
        workloads=[
            {"type": "web_app", "count": 10, "replicas": {"min": 5, "max": 50},
             "churn": "high", "traffic": "steady",
             "pdb": {"min_available": "80%"},
             "topology_spread": {"max_skew": 1, "topology_key": "topology.kubernetes.io/zone"}},
        ],
        traffic_pattern={"type": "spike", "peak_multiplier": 3.0, "duration": "24h"},
    )


def topology_deadlock(zones: int = 2, max_skew: int = 1, replicas: int = 10) -> dict:
    """Can't satisfy topology spread — too many replicas for available zones."""
    return _base_scenario(
        "topology-deadlock",
        pools=[{"instance_types": ["m5.large"], "min_nodes": 1, "max_nodes": replicas}],
        workloads=[{
            "type": "web_app", "count": 1,
            "replicas": {"min": replicas, "max": replicas},
            "churn": "low", "traffic": "steady",
            "topology_spread": {"max_skew": max_skew, "topology_key": "topology.kubernetes.io/zone"},
            "pdb": {"min_available": "80%"},
        }],
    )


def mixed_workload_contention(batch_pct: float = 0.8, web_pct: float = 0.2, total_pods: int = 100) -> dict:
    """Resource competition between batch and web workloads."""
    batch_count = max(1, int(total_pods * batch_pct))
    web_count = max(1, int(total_pods * web_pct))
    return _base_scenario(
        "mixed-workload-contention",
        pools=[{"instance_types": ["m5.xlarge", "c5.xlarge"], "min_nodes": 1, "max_nodes": total_pods}],
        workloads=[
            {"type": "batch_job", "count": batch_count, "priority": "low",
             "cpu_request": {"dist": "uniform", "min": "500m", "max": "2000m"},
             "duration": {"dist": "exponential", "mean": "30m"}},
            {"type": "web_app", "count": web_count,
             "replicas": {"min": 2, "max": 20}, "churn": "medium", "traffic": "diurnal",
             "pdb": {"min_available": "50%"}},
        ],
    )


def single_pool_saturation(instance_type: str = "m5.large", max_nodes: int = 10, pod_count: int = 50) -> dict:
    """Single instance type pool pushed to its node limit."""
    return _base_scenario(
        "single-pool-saturation",
        pools=[{"instance_types": [instance_type], "min_nodes": 1, "max_nodes": max_nodes}],
        workloads=[{
            "type": "web_app", "count": 1,
            "replicas": {"min": pod_count, "max": pod_count},
            "churn": "low", "traffic": "steady",
            "cpu_request": {"dist": "uniform", "min": "250m", "max": "500m"},
        }],
    )


# Registry for name-based lookup
TEMPLATES: dict[str, callable] = {
    "bin_packing_stress": bin_packing_stress,
    "consolidation_cascade": consolidation_cascade,
    "spot_interruption_storm": spot_interruption_storm,
    "topology_deadlock": topology_deadlock,
    "mixed_workload_contention": mixed_workload_contention,
    "single_pool_saturation": single_pool_saturation,
}
