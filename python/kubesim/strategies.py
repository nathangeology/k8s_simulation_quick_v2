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
    "c5.12xlarge", "c5.16xlarge", "c5.24xlarge", "c5.2xlarge", "c5.4xlarge", "c5.8xlarge",
    "c5.large", "c5.xlarge", "c5a.12xlarge", "c5a.16xlarge", "c5a.24xlarge", "c5a.2xlarge",
    "c5a.4xlarge", "c5a.8xlarge", "c5a.large", "c5a.xlarge", "c5n.18xlarge", "c5n.2xlarge",
    "c5n.4xlarge", "c5n.9xlarge", "c5n.large", "c5n.xlarge", "c6a.12xlarge", "c6a.16xlarge",
    "c6a.24xlarge", "c6a.2xlarge", "c6a.4xlarge", "c6a.8xlarge", "c6a.large", "c6a.xlarge",
    "c6i.12xlarge", "c6i.16xlarge", "c6i.24xlarge", "c6i.2xlarge", "c6i.4xlarge", "c6i.8xlarge",
    "c6i.large", "c6i.xlarge", "c7a.12xlarge", "c7a.16xlarge", "c7a.24xlarge", "c7a.2xlarge",
    "c7a.4xlarge", "c7a.8xlarge", "c7a.large", "c7a.xlarge", "c7i.12xlarge", "c7i.16xlarge",
    "c7i.24xlarge", "c7i.2xlarge", "c7i.4xlarge", "c7i.8xlarge", "c7i.large", "c7i.xlarge",
    "d3.2xlarge", "d3.4xlarge", "d3.8xlarge", "d3.xlarge", "hpc6a.48xlarge", "hpc7g.16xlarge",
    "hpc7g.4xlarge", "hpc7g.8xlarge", "i3.16xlarge", "i3.2xlarge", "i3.4xlarge", "i3.8xlarge",
    "i3.large", "i3.xlarge", "i4i.16xlarge", "i4i.2xlarge", "i4i.32xlarge", "i4i.4xlarge",
    "i4i.8xlarge", "i4i.large", "i4i.xlarge", "m5.12xlarge", "m5.16xlarge", "m5.24xlarge",
    "m5.2xlarge", "m5.4xlarge", "m5.8xlarge", "m5.large", "m5.xlarge", "m5a.12xlarge",
    "m5a.16xlarge", "m5a.24xlarge", "m5a.2xlarge", "m5a.4xlarge", "m5a.8xlarge", "m5a.large",
    "m5a.xlarge", "m6a.12xlarge", "m6a.16xlarge", "m6a.24xlarge", "m6a.2xlarge", "m6a.4xlarge",
    "m6a.8xlarge", "m6a.large", "m6a.xlarge", "m6i.12xlarge", "m6i.16xlarge", "m6i.24xlarge",
    "m6i.2xlarge", "m6i.4xlarge", "m6i.8xlarge", "m6i.large", "m6i.xlarge", "m7a.12xlarge",
    "m7a.16xlarge", "m7a.24xlarge", "m7a.2xlarge", "m7a.4xlarge", "m7a.8xlarge", "m7a.large",
    "m7a.xlarge", "m7i.12xlarge", "m7i.16xlarge", "m7i.24xlarge", "m7i.2xlarge", "m7i.4xlarge",
    "m7i.8xlarge", "m7i.large", "m7i.xlarge", "r5.12xlarge", "r5.16xlarge", "r5.24xlarge",
    "r5.2xlarge", "r5.4xlarge", "r5.8xlarge", "r5.large", "r5.xlarge", "r5a.12xlarge",
    "r5a.16xlarge", "r5a.24xlarge", "r5a.2xlarge", "r5a.4xlarge", "r5a.8xlarge", "r5a.large",
    "r5a.xlarge", "r5n.16xlarge", "r5n.24xlarge", "r5n.2xlarge", "r5n.4xlarge", "r5n.8xlarge",
    "r5n.large", "r5n.xlarge", "r6a.12xlarge", "r6a.16xlarge", "r6a.24xlarge", "r6a.2xlarge",
    "r6a.4xlarge", "r6a.8xlarge", "r6a.large", "r6a.xlarge", "r6i.12xlarge", "r6i.16xlarge",
    "r6i.24xlarge", "r6i.2xlarge", "r6i.4xlarge", "r6i.8xlarge", "r6i.large", "r6i.xlarge",
    "r7a.12xlarge", "r7a.16xlarge", "r7a.24xlarge", "r7a.2xlarge", "r7a.4xlarge", "r7a.8xlarge",
    "r7a.large", "r7a.xlarge", "r7i.12xlarge", "r7i.16xlarge", "r7i.24xlarge", "r7i.2xlarge",
    "r7i.4xlarge", "r7i.8xlarge", "r7i.large", "r7i.xlarge", "t3.2xlarge", "t3.large",
    "t3.medium", "t3.micro", "t3.nano", "t3.small", "t3.xlarge", "x2idn.16xlarge",
    "x2idn.24xlarge", "x2idn.32xlarge", "z1d.12xlarge", "z1d.2xlarge", "z1d.3xlarge",
    "z1d.6xlarge", "z1d.large", "z1d.xlarge",
]

TINY_INSTANCE_TYPES = ["t3.nano", "t3.micro", "t3.small", "t3.medium"]

GPU_INSTANCE_TYPES = [
    "g5.xlarge", "g5.2xlarge", "g5.4xlarge", "g5.8xlarge", "g5.12xlarge", "g5.16xlarge",
    "g5.24xlarge", "g5.48xlarge", "g6.xlarge", "g6.2xlarge", "g6.4xlarge", "g6.8xlarge",
    "g6.12xlarge", "g6.16xlarge", "g6.24xlarge", "g6.48xlarge", "inf2.xlarge", "inf2.8xlarge",
    "inf2.24xlarge", "inf2.48xlarge", "p4d.24xlarge", "p5.48xlarge", "trn1.2xlarge",
    "trn1.32xlarge",
]

WORKLOAD_TYPES = ["web_app", "ml_training", "batch_job", "saas_microservice"]

EDGE_CASE_WORKLOAD_TYPES = [
    "gpu_on_non_gpu", "extreme_replicas", "overcommit",
    "anti_affinity", "varying_batch",
]

ALL_WORKLOAD_TYPES = WORKLOAD_TYPES + EDGE_CASE_WORKLOAD_TYPES

SCALE_DOWN_PATTERNS = ["cliff", "staggered", "oscillating"]

TOPOLOGY_KEYS = [
    "topology.kubernetes.io/zone",
    "kubernetes.io/hostname",
]

CONSOLIDATION_POLICIES = ["WhenEmpty", "WhenUnderutilized"]

TRAFFIC_PATTERNS = ["diurnal", "spike", "steady", "diurnal_with_spike"]

CHURN_LEVELS = ["low", "medium", "high"]

PRIORITY_LEVELS = ["low", "medium", "high", "critical"]

SCALING_METRICS = ["cpu", "memory", "rps"]

SEARCH_RUNS = 50  # Fixed during adversarial search; report phase uses higher counts


# ── Leaf strategies ──────────────────────────────────────────────

def _cpu_dist() -> SearchStrategy[dict]:
    """Strategy for a CPU request — fixed value per deployment.

    Real deployments have a specific CPU request (e.g. 250m, 500m, 2000m),
    not a distribution. Different deployments (count > 1) get different
    values sampled from this strategy, but all replicas within a deployment
    share the same request via the pod template.
    """
    return st.builds(
        lambda m: {"dist": "uniform", "min": f"{m}m", "max": f"{m}m"},
        m=st.sampled_from([100, 250, 500, 750, 1000, 1500, 2000, 4000, 8000]),
    )


def _memory_dist() -> SearchStrategy[dict]:
    """Strategy for a memory request — fixed value per deployment."""
    return st.builds(
        lambda m: {"dist": "uniform", "min": f"{m}Mi", "max": f"{m}Mi"},
        m=st.sampled_from([128, 256, 512, 1024, 2048, 4096, 8192, 16384]),
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


def pod_anti_affinity() -> SearchStrategy[dict]:
    """Strategy for pod anti-affinity (pods that can't co-locate)."""
    return st.fixed_dictionaries({
        "topology_key": st.sampled_from(["kubernetes.io/hostname", "topology.kubernetes.io/zone"]),
    })


def scheduling_constraints() -> SearchStrategy[dict]:
    """Strategy for optional scheduling constraints on a workload."""
    return st.fixed_dictionaries({}, optional={
        "topology_spread": topology_spread(),
        "pdb": pdb(),
        "node_affinity": st.lists(node_affinity_term(), min_size=1, max_size=2),
        "pod_anti_affinity": pod_anti_affinity(),
    })


# ── Edge-case node pool strategies ──────────────────────────────

def single_instance_pool(
    *,
    instance_type: SearchStrategy[str] | None = None,
    max_nodes: SearchStrategy[int] | None = None,
) -> SearchStrategy[dict]:
    """Single instance type pool — forces exact bin-packing, no fallback."""
    return st.fixed_dictionaries({
        "instance_types": (instance_type or st.sampled_from(INSTANCE_TYPES)).map(lambda t: [t]),
        "min_nodes": st.just(1),
        "max_nodes": max_nodes or st.integers(5, 100),
    })


def tiny_instance_pool() -> SearchStrategy[dict]:
    """Tiny instances (t3.micro/small) — stress bin-packing with large pods."""
    return st.fixed_dictionaries({
        "instance_types": st.sampled_from(TINY_INSTANCE_TYPES).map(lambda t: [t]),
        "min_nodes": st.just(1),
        "max_nodes": st.integers(10, 200),
    })


def mixed_spot_ondemand_pools() -> SearchStrategy[list[dict]]:
    """Mixed spot/on-demand pools with different instance families."""
    spot = st.fixed_dictionaries({
        "instance_types": st.lists(st.sampled_from(["m5.xlarge", "m5.2xlarge", "c5.xlarge"]), min_size=2, max_size=3, unique=True),
        "min_nodes": st.just(0),
        "max_nodes": st.integers(10, 100),
    })
    ondemand = st.fixed_dictionaries({
        "instance_types": st.lists(st.sampled_from(["m6i.large", "m6i.xlarge", "r5.large"]), min_size=1, max_size=2, unique=True),
        "min_nodes": st.integers(1, 5),
        "max_nodes": st.integers(10, 50),
    })
    return st.tuples(spot, ondemand).map(list)


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


def _gpu_on_non_gpu_workload() -> SearchStrategy[dict]:
    """GPU pods targeting non-GPU pools — should fail gracefully."""
    return st.fixed_dictionaries({
        "type": st.just("ml_training"),
        "count": st.integers(1, 5),
        "replicas": st.fixed_dictionaries({"fixed": st.just(1)}),
        "priority": st.just("high"),
        "gpu_request": _gpu_dist(),
        "cpu_request": _cpu_dist(),
        "memory_request": _memory_dist(),
    })


def _extreme_replica_workload() -> SearchStrategy[dict]:
    """Extreme replica counts — 1 replica with PDB or 500 replicas."""
    return st.one_of(
        # Single replica with strict PDB
        st.fixed_dictionaries({
            "type": st.just("web_app"),
            "count": st.just(1),
            "replicas": st.fixed_dictionaries({"min": st.just(1), "max": st.just(1)}),
            "churn": st.just("low"), "traffic": st.just("steady"),
            "pdb": st.just({"min_available": "1"}),
        }),
        # Massive replica count
        st.fixed_dictionaries({
            "type": st.just("web_app"),
            "count": st.just(1),
            "replicas": st.fixed_dictionaries({"min": st.integers(200, 500), "max": st.integers(500, 1000)}),
            "churn": st.sampled_from(["low", "medium"]),
            "traffic": st.just("steady"),
        }),
    )


def _overcommit_workload() -> SearchStrategy[dict]:
    """Workloads requesting more resources than cluster can provide."""
    return st.fixed_dictionaries({
        "type": st.just("batch_job"),
        "count": st.integers(20, 100),
        "priority": st.sampled_from(["low", "medium", "high"]),
        "cpu_request": st.builds(
            lambda m: {"dist": "uniform", "min": f"{m}m", "max": f"{m}m"},
            m=st.sampled_from([4000, 6000, 8000, 12000, 16000]),
        ),
        "memory_request": st.builds(
            lambda m: {"dist": "uniform", "min": f"{m}Mi", "max": f"{m}Mi"},
            m=st.sampled_from([16384, 32768, 65536]),
        ),
        "duration": _duration_dist(),
    })


def _anti_affinity_workload() -> SearchStrategy[dict]:
    """Pods with anti-affinity that can't co-locate."""
    return st.fixed_dictionaries({
        "type": st.just("web_app"),
        "count": st.integers(1, 5),
        "replicas": st.fixed_dictionaries({"min": st.integers(3, 20), "max": st.integers(20, 50)}),
        "churn": st.just("low"), "traffic": st.just("steady"),
        "pod_anti_affinity": st.just({"topology_key": "kubernetes.io/hostname"}),
        "topology_spread": st.just({"max_skew": 1, "topology_key": "topology.kubernetes.io/zone"}),
    })


def _varying_batch_workload() -> SearchStrategy[dict]:
    """Batch jobs with varying lifetimes (1min to 24h)."""
    return st.fixed_dictionaries({
        "type": st.just("batch_job"),
        "count": st.integers(5, 50),
        "priority": st.sampled_from(["low", "medium"]),
        "cpu_request": _cpu_dist(),
        "memory_request": _memory_dist(),
        "duration": st.one_of(
            st.just({"dist": "exponential", "mean": "1m"}),
            st.just({"dist": "exponential", "mean": "60m"}),
            st.just({"dist": "lognormal", "mean": "12h", "std": "6h"}),
        ),
    })


_ARCHETYPE_STRATEGIES = {
    "web_app": _web_app_workload,
    "ml_training": _ml_training_workload,
    "batch_job": _batch_job_workload,
    "saas_microservice": _saas_microservice_workload,
    "gpu_on_non_gpu": _gpu_on_non_gpu_workload,
    "extreme_replicas": _extreme_replica_workload,
    "overcommit": _overcommit_workload,
    "anti_affinity": _anti_affinity_workload,
    "varying_batch": _varying_batch_workload,
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
    """Strategy for a single node pool definition.

    By default, ~80% of pools use all available instance types (realistic),
    and ~20% use a restricted subset (to explore constrained scenarios).
    """
    if instance_types is None:
        # Use an integer draw to control the branch: 0-3 → all types, 4 → restricted
        @st.composite
        def _instance_types(draw):
            branch = draw(st.integers(0, 4))
            if branch < 4:
                return list(INSTANCE_TYPES)
            return draw(st.lists(st.sampled_from(INSTANCE_TYPES), min_size=1, max_size=6, unique=True))
        instance_types = _instance_types()
    return st.fixed_dictionaries({
        "instance_types": instance_types,
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
                "runs": SEARCH_RUNS,
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


def chaos_scenario(
    *,
    max_nodes: int = 100,
) -> SearchStrategy[dict]:
    """Worst-case chaos scenario: single-instance pools + overcommit + topology + edge-case workloads.

    Combines the hardest config dimensions to maximize divergence between strategies.
    """
    @st.composite
    def _build(draw):
        # Single-instance pool (no fallback)
        pool = draw(single_instance_pool(max_nodes=st.integers(5, max_nodes)))
        # Optionally add a tiny instance pool
        pools = [pool]
        if draw(st.booleans()):
            pools.append(draw(tiny_instance_pool()))

        # Mix edge-case and normal workloads
        edge_wls = draw(st.lists(
            workload(workload_types=EDGE_CASE_WORKLOAD_TYPES), min_size=1, max_size=3,
        ))
        normal_wls = draw(st.lists(
            workload(workload_types=WORKLOAD_TYPES), min_size=1, max_size=3,
        ))

        scenario = {
            "study": {
                "name": "chaos-" + draw(st.text(
                    alphabet="abcdefghijklmnopqrstuvwxyz0123456789",
                    min_size=3, max_size=10,
                )),
                "runs": SEARCH_RUNS,
                "time_mode": "logical",
                "cluster": {"node_pools": pools},
                "workloads": edge_wls + normal_wls,
                "scale_down_pattern": draw(st.sampled_from(SCALE_DOWN_PATTERNS)),
                "variants": [],
                "metrics": {"compare": []},
            }
        }
        # Always include traffic for chaos
        scenario["study"]["traffic_pattern"] = draw(traffic_pattern())
        return scenario

    return _build()
