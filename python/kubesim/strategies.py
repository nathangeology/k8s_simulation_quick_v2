"""Hypothesis composite strategies for KubeSim scenario generation.

Generates random but valid cluster scenarios as Python dicts matching the
YAML scenario schema (ScenarioFile). Strategies are composable — users can
constrain specific dimensions while letting others vary freely.

Usage with Hypothesis::

    from hypothesis import given
    from kubesim.strategies import cluster_scenario

    @given(scenario=cluster_scenario(max_nodes=500))
    def test_no_pod_starved(scenario):
        result = kubesim.run(scenario)
        assert result.max_pending_duration < timedelta(minutes=5)
"""

from __future__ import annotations

from hypothesis import strategies as st
from hypothesis.strategies import SearchStrategy

# ── Constants ────────────────────────────────────────────────────

INSTANCE_TYPES = [
    "m5.large", "m5.xlarge", "m5.2xlarge", "m5.4xlarge",
    "c5.large", "c5.xlarge", "c5.2xlarge", "c5.4xlarge",
    "r5.large", "r5.xlarge", "r5.2xlarge",
    "m6i.large", "m6i.xlarge", "m6i.2xlarge",
    "c6i.large", "c6i.xlarge", "c6i.2xlarge",
    "p3.2xlarge", "p3.8xlarge", "g4dn.xlarge", "g4dn.2xlarge",
]

GPU_INSTANCE_TYPES = ["p3.2xlarge", "p3.8xlarge", "g4dn.xlarge", "g4dn.2xlarge"]

WORKLOAD_TYPES = ["web_app", "ml_training", "batch_job", "saas_microservice"]

TOPOLOGY_KEYS = [
    "topology.kubernetes.io/zone",
    "kubernetes.io/hostname",
]

CONSOLIDATION_POLICIES = ["WhenEmpty", "WhenUnderutilized"]

TRAFFIC_PATTERNS = ["diurnal", "spike", "steady", "diurnal_with_spike"]

CHURN_LEVELS = ["low", "medium", "high"]

PRIORITY_LEVELS = ["low", "medium", "high", "critical"]

SCALING_METRICS = ["cpu", "memory", "rps"]


# ── Leaf strategies ──────────────────────────────────────────────

def _cpu_dist() -> SearchStrategy[dict]:
    """Strategy for a CPU request distribution."""
    return st.one_of(
        st.builds(
            lambda lo, hi: {"dist": "uniform", "min": f"{lo}m", "max": f"{hi}m"},
            lo=st.integers(50, 2000),
            hi=st.integers(2001, 32000),
        ),
        st.builds(
            lambda mean, std: {"dist": "normal", "mean": f"{mean}m", "std": f"{std}m"},
            mean=st.integers(100, 8000),
            std=st.integers(50, 2000),
        ),
    )


def _memory_dist() -> SearchStrategy[dict]:
    """Strategy for a memory request distribution."""
    return st.one_of(
        st.builds(
            lambda lo, hi: {"dist": "uniform", "min": f"{lo}Mi", "max": f"{hi}Mi"},
            lo=st.integers(64, 4096),
            hi=st.integers(4097, 131072),
        ),
        st.builds(
            lambda mean, std: {"dist": "normal", "mean": f"{mean}Mi", "std": f"{std}Mi"},
            mean=st.integers(128, 32768),
            std=st.integers(32, 8192),
        ),
    )


def _gpu_dist() -> SearchStrategy[dict]:
    """Strategy for a GPU request distribution."""
    return st.just({"dist": "choice", "values": [1, 2, 4, 8]})


def _duration_dist() -> SearchStrategy[dict]:
    """Strategy for a workload duration distribution."""
    return st.one_of(
        st.builds(
            lambda mean: {"dist": "exponential", "mean": f"{mean}m"},
            mean=st.integers(5, 480),
        ),
        st.builds(
            lambda mean, std: {"dist": "lognormal", "mean": f"{mean}h", "std": f"{std}h"},
            mean=st.integers(1, 24),
            std=st.integers(1, 12),
        ),
    )


# ── Scheduling constraints ───────────────────────────────────────

def topology_spread(
    *,
    max_skew: SearchStrategy[int] | None = None,
    topology_key: SearchStrategy[str] | None = None,
) -> SearchStrategy[dict]:
    """Strategy for a TopologySpreadConstraint."""
    return st.fixed_dictionaries({
        "max_skew": max_skew if max_skew is not None else st.integers(1, 5),
        "topology_key": topology_key if topology_key is not None else st.sampled_from(TOPOLOGY_KEYS),
    })


def pdb(
    *,
    min_available: SearchStrategy[str] | None = None,
) -> SearchStrategy[dict]:
    """Strategy for a PodDisruptionBudget."""
    return st.fixed_dictionaries({
        "min_available": min_available if min_available is not None else st.sampled_from(["1", "2", "25%", "50%", "80%"]),
    })


def node_affinity_term() -> SearchStrategy[dict]:
    """Strategy for a node affinity label match."""
    return st.fixed_dictionaries({
        "key": st.sampled_from(["topology.kubernetes.io/zone", "node.kubernetes.io/instance-type"]),
        "value": st.sampled_from(["us-east-1a", "us-east-1b", "us-west-2a", "m5.xlarge", "c5.xlarge"]),
    })


def scheduling_constraints() -> SearchStrategy[dict]:
    """Strategy for optional scheduling constraints on a workload."""
    return st.fixed_dictionaries({}, optional={
        "topology_spread": topology_spread(),
        "pdb": pdb(),
        "node_affinity": st.lists(node_affinity_term(), min_size=1, max_size=2),
    })


# ── Workload strategies ─────────────────────────────────────────

def _web_app_workload() -> SearchStrategy[dict]:
    return st.fixed_dictionaries({
        "type": st.just("web_app"),
        "count": st.integers(1, 20),
        "replicas": st.fixed_dictionaries({"min": st.integers(2, 10), "max": st.integers(11, 50)}),
        "churn": st.just("low"),
        "traffic": st.just("diurnal"),
    }, optional={
        "cpu_request": _cpu_dist(),
        "memory_request": _memory_dist(),
        "scaling": st.just({"type": "hpa", "metric": "cpu", "target": "70%"}),
        "topology_spread": topology_spread(),
        "pdb": pdb(),
    })


def _ml_training_workload() -> SearchStrategy[dict]:
    return st.fixed_dictionaries({
        "type": st.just("ml_training"),
        "count": st.integers(1, 10),
        "replicas": st.fixed_dictionaries({"fixed": st.just(1)}),
        "priority": st.just("high"),
    }, optional={
        "cpu_request": _cpu_dist(),
        "memory_request": _memory_dist(),
        "gpu_request": _gpu_dist(),
        "duration": _duration_dist(),
    })


def _batch_job_workload() -> SearchStrategy[dict]:
    return st.fixed_dictionaries({
        "type": st.just("batch_job"),
        "count": st.integers(1, 30),
        "priority": st.just("low"),
    }, optional={
        "cpu_request": _cpu_dist(),
        "memory_request": _memory_dist(),
        "duration": _duration_dist(),
    })


def _saas_microservice_workload() -> SearchStrategy[dict]:
    return st.fixed_dictionaries({
        "type": st.just("saas_microservice"),
        "count": st.integers(1, 15),
        "replicas": st.fixed_dictionaries({"min": st.integers(3, 10), "max": st.integers(11, 200)}),
        "churn": st.sampled_from(["low", "medium"]),
    }, optional={
        "cpu_request": _cpu_dist(),
        "memory_request": _memory_dist(),
        "scaling": st.just({"type": "hpa", "metric": "rps", "target": 1000}),
        "topology_spread": topology_spread(),
        "pdb": pdb(),
    })


_ARCHETYPE_STRATEGIES = {
    "web_app": _web_app_workload,
    "ml_training": _ml_training_workload,
    "batch_job": _batch_job_workload,
    "saas_microservice": _saas_microservice_workload,
}


def workload(
    *,
    workload_types: list[str] | None = None,
) -> SearchStrategy[dict]:
    """Strategy for a single workload definition.

    Args:
        workload_types: Restrict to these archetype names. Defaults to all four.
    """
    types = workload_types or WORKLOAD_TYPES
    return st.sampled_from([_ARCHETYPE_STRATEGIES[t] for t in types]).flatmap(lambda f: f())


def workload_mix(
    *,
    workload_types: list[str] | None = None,
    min_workloads: int = 1,
    max_workloads: int = 8,
) -> SearchStrategy[list[dict]]:
    """Strategy for a list of workload definitions."""
    return st.lists(
        workload(workload_types=workload_types),
        min_size=min_workloads,
        max_size=max_workloads,
    )


# ── Node pool strategies ────────────────────────────────────────

def node_pool(
    *,
    instance_types: SearchStrategy[list[str]] | None = None,
    min_nodes: SearchStrategy[int] | None = None,
    max_nodes: SearchStrategy[int] | None = None,
    karpenter: SearchStrategy[dict | None] | None = None,
) -> SearchStrategy[dict]:
    """Strategy for a single node pool definition."""
    return st.fixed_dictionaries({
        "instance_types": instance_types if instance_types is not None else st.lists(
            st.sampled_from(INSTANCE_TYPES), min_size=1, max_size=6, unique=True,
        ),
        "min_nodes": min_nodes if min_nodes is not None else st.integers(1, 10),
        "max_nodes": max_nodes if max_nodes is not None else st.integers(11, 200),
    }, optional={
        "karpenter": karpenter if karpenter is not None else st.fixed_dictionaries({
            "consolidation": st.fixed_dictionaries({
                "policy": st.sampled_from(CONSOLIDATION_POLICIES),
            }),
        }),
    })


def node_pools(
    *,
    min_pools: int = 1,
    max_pools: int = 3,
    **kwargs,
) -> SearchStrategy[list[dict]]:
    """Strategy for a list of node pool definitions."""
    return st.lists(node_pool(**kwargs), min_size=min_pools, max_size=max_pools)


# ── Traffic pattern strategies ───────────────────────────────────

def traffic_pattern(
    *,
    pattern_types: list[str] | None = None,
) -> SearchStrategy[dict]:
    """Strategy for a traffic pattern."""
    types = pattern_types or TRAFFIC_PATTERNS
    return st.fixed_dictionaries({
        "type": st.sampled_from(types),
        "peak_multiplier": st.floats(1.5, 10.0, allow_nan=False),
        "duration": st.sampled_from(["12h", "24h", "48h"]),
    })


# ── Variant strategies ───────────────────────────────────────────

def scheduler_variant(name: str, scoring: str) -> SearchStrategy[dict]:
    """Strategy for a scheduler variant (fixed scoring strategy)."""
    return st.just({"name": name, "scheduler": {"scoring": scoring, "weight": 1}})


def deletion_cost_variant(name: str, strategy: str) -> SearchStrategy[dict]:
    """Strategy for a deletion cost variant."""
    base = {"name": name, "deletion_cost_strategy": strategy}
    return st.just(base)


# ── Top-level scenario strategy ──────────────────────────────────

def cluster_scenario(
    *,
    max_nodes: int | None = None,
    workload_types: list[str] | None = None,
    min_workloads: int = 1,
    max_workloads: int = 8,
    traffic: bool | None = None,
    min_pools: int = 1,
    max_pools: int = 3,
) -> SearchStrategy[dict]:
    """Composite strategy generating a complete cluster scenario dict.

    The returned dict matches the ScenarioFile YAML schema and can be
    serialized to YAML for ``kubesim.run()`` or used directly.

    Args:
        max_nodes: Cap max_nodes per pool. Defaults to 200.
        workload_types: Restrict workload archetypes.
        min_workloads: Minimum number of workloads.
        max_workloads: Maximum number of workloads.
        traffic: Force traffic pattern on/off. None = random.
        min_pools: Minimum node pools.
        max_pools: Maximum node pools.
    """
    cap = max_nodes or 200
    pool_strat = node_pools(
        min_pools=min_pools,
        max_pools=max_pools,
        max_nodes=st.integers(11, cap),
    )
    workloads_strat = workload_mix(
        workload_types=workload_types,
        min_workloads=min_workloads,
        max_workloads=max_workloads,
    )

    if traffic is True:
        traffic_strat = st.just(True)
    elif traffic is False:
        traffic_strat = st.just(False)
    else:
        traffic_strat = st.booleans()

    @st.composite
    def _build(draw):
        pools = draw(pool_strat)
        wls = draw(workloads_strat)
        include_traffic = draw(traffic_strat)

        scenario = {
            "study": {
                "name": draw(st.text(
                    alphabet="abcdefghijklmnopqrstuvwxyz0123456789-",
                    min_size=3, max_size=30,
                )),
                "runs": draw(st.integers(1, 10000)),
                "time_mode": draw(st.sampled_from(["logical", "wall_clock"])),
                "cluster": {"node_pools": pools},
                "workloads": wls,
                "variants": [],
                "metrics": {"compare": []},
            }
        }
        if include_traffic:
            scenario["study"]["traffic_pattern"] = draw(traffic_pattern())
        return scenario

    return _build()
