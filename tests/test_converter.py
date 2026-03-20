"""Tests for the scenario conversion framework."""

import re
import textwrap
import tempfile
from pathlib import Path

import yaml
import pytest

from kubesim.converter.base import (
    Cluster,
    ConversionMetadata,
    DaemonSet,
    Delays,
    NodePool,
    ScenarioIR,
    Workload,
)
from kubesim.converter.k8s_import import K8sImportAdapter, _normalize_cpu, _normalize_memory
from kubesim.converter.renderer import render_study_yaml

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

FIXTURE_DIR = Path(__file__).resolve().parent.parent / "scenarios" / "needs_conversion" / "k8s_import_grafana_1752595626"
GOLDEN_REF = Path(__file__).resolve().parent.parent / "scenarios" / "grafana-import-01.yaml"


def _make_mini_scenario(tmp_path: Path, *, deployments: list[dict] | None = None,
                        steps: str | None = None, config_overrides: dict | None = None) -> Path:
    """Create a minimal k8s-import scenario directory for testing."""
    scenario_dir = tmp_path / "test_scenario"
    scenario_dir.mkdir()
    deploy_dir = scenario_dir / "deployments"
    deploy_dir.mkdir()

    if deployments is None:
        deployments = [
            {"name": "web", "cpu": "0.5", "memory": "512Mi", "replicas": 2},
        ]

    dep_names = []
    for dep in deployments:
        name = dep["name"]
        dep_names.append(name)
        spec: dict = {
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {"name": name},
            "spec": {
                "replicas": dep.get("replicas", 1),
                "selector": {"matchLabels": {"app": name}},
                "template": {
                    "metadata": {"labels": {"app": name}},
                    "spec": {
                        "terminationGracePeriodSeconds": 30,
                        "containers": [{
                            "name": f"{name}-container",
                            "image": "nginx:latest",
                            "resources": {"requests": {
                                "cpu": dep.get("cpu", "0.25"),
                                "memory": dep.get("memory", "256Mi"),
                            }},
                        }],
                    },
                },
            },
        }
        # Add affinity if specified
        if "affinity" in dep:
            spec["spec"]["template"]["spec"]["affinity"] = dep["affinity"]
        if "topologySpreadConstraints" in dep:
            spec["spec"]["template"]["spec"]["topologySpreadConstraints"] = dep["topologySpreadConstraints"]

        (deploy_dir / f"{name}.yaml").write_text(yaml.dump(spec))

    config = {
        "simulator": {
            "run_id": "test",
            "timestep": 60,
            "start_step": 1,
            "limit": 60,
            "instance_pool_size": 1000,
            "clusters": [{"KubernetesCluster": {
                "type": "KubernetesCluster",
                "name": "default",
                "node_count": 3,
                "node_type": "m5.large",
                "autoscaling": True,
            }}],
            "deployments_directory": "deployments",
            "deployments": dep_names,
        },
    }
    if config_overrides:
        config["simulator"].update(config_overrides)
    (scenario_dir / "config.yml").write_text(yaml.dump(config))

    if steps is None:
        steps = "scenario: []\n"
    (scenario_dir / "steps.yml").write_text(steps)

    return scenario_dir


# ---------------------------------------------------------------------------
# Unit tests: normalization
# ---------------------------------------------------------------------------

class TestNormalization:
    def test_cpu_from_float(self):
        assert _normalize_cpu("0.5") == "500m"

    def test_cpu_already_millicores(self):
        assert _normalize_cpu("250m") == "250m"

    def test_cpu_integer(self):
        assert _normalize_cpu("2.0") == "2000m"

    def test_memory_already_mi(self):
        assert _normalize_memory("512Mi") == "512Mi"

    def test_memory_from_gi(self):
        assert _normalize_memory("6Gi") == "6144Mi"


# ---------------------------------------------------------------------------
# Unit tests: k8s-import adapter
# ---------------------------------------------------------------------------

class TestK8sImportAdapter:
    def test_basic_conversion(self, tmp_path):
        scenario_dir = _make_mini_scenario(tmp_path)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)

        assert ir.name == "test_scenario"
        assert len(ir.workloads) == 1
        assert ir.workloads[0].name == "web"
        assert ir.workloads[0].cpu_request == "500m"
        assert ir.workloads[0].memory_request == "512Mi"
        assert ir.workloads[0].initial_replicas == 2

    def test_scaling_timeline(self, tmp_path):
        steps = textwrap.dedent("""\
            scenario:
            - step:
                name: 1
                actions:
                - action:
                    comment: Scales deployment web to 5 replicas
                    action_type: K8S_SCALE
                    action_data: name=web,replicas=5
            - step:
                name: 3
                actions:
                - action:
                    comment: Scales deployment web to 2 replicas
                    action_type: K8S_SCALE
                    action_data: name=web,replicas=2
        """)
        scenario_dir = _make_mini_scenario(tmp_path, steps=steps)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)

        w = ir.workloads[0]
        assert w.scaling_timeline == [(1, 5), (3, 2)]

    def test_affinity_parsing(self, tmp_path):
        deployments = [{
            "name": "ingester",
            "cpu": "2.0",
            "memory": "6144Mi",
            "affinity": {
                "podAntiAffinity": {
                    "requiredDuringSchedulingIgnoredDuringExecution": [{
                        "labelSelector": {
                            "matchExpressions": [{
                                "key": "rollout-group",
                                "operator": "In",
                                "values": ["ingester"],
                            }],
                        },
                        "topologyKey": "kubernetes.io/hostname",
                    }],
                },
            },
        }]
        scenario_dir = _make_mini_scenario(tmp_path, deployments=deployments)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)

        w = ir.workloads[0]
        assert w.pod_anti_affinity is not None
        assert w.pod_anti_affinity["label_key"] == "rollout-group"
        assert w.pod_anti_affinity["affinity_type"] == "required"

    def test_topology_spread_parsing(self, tmp_path):
        deployments = [{
            "name": "spread-app",
            "cpu": "0.5",
            "memory": "512Mi",
            "topologySpreadConstraints": [{
                "labelSelector": {"matchLabels": {"name": "spread-app"}},
                "maxSkew": 1,
                "topologyKey": "kubernetes.io/hostname",
                "whenUnsatisfiable": "ScheduleAnyway",
            }],
        }]
        scenario_dir = _make_mini_scenario(tmp_path, deployments=deployments)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)

        w = ir.workloads[0]
        assert w.topology_spread is not None
        assert w.topology_spread["max_skew"] == 1

    def test_no_scaling_events(self, tmp_path):
        """Deployment with no entries in steps.yml gets empty timeline."""
        scenario_dir = _make_mini_scenario(tmp_path)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)
        assert ir.workloads[0].scaling_timeline == []

    def test_zero_replica_deployment(self, tmp_path):
        deployments = [{"name": "idle", "cpu": "0.1", "memory": "128Mi", "replicas": 0}]
        scenario_dir = _make_mini_scenario(tmp_path, deployments=deployments)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)
        assert ir.workloads[0].initial_replicas == 0

    def test_multiple_deployments_sorted(self, tmp_path):
        deployments = [
            {"name": "zebra", "cpu": "0.1", "memory": "128Mi"},
            {"name": "alpha", "cpu": "0.2", "memory": "256Mi"},
        ]
        scenario_dir = _make_mini_scenario(tmp_path, deployments=deployments)
        adapter = K8sImportAdapter()
        ir = adapter.convert(scenario_dir)
        assert [w.name for w in ir.workloads] == ["alpha", "zebra"]


# ---------------------------------------------------------------------------
# Unit tests: renderer
# ---------------------------------------------------------------------------

class TestRenderer:
    def test_basic_render(self):
        ir = ScenarioIR(
            name="test",
            cluster=Cluster(
                node_pools=[NodePool(instance_types=["m5.large"])],
                daemonsets=[DaemonSet("kube-proxy", "100m", "256Mi")],
            ),
            workloads=[Workload(name="web", cpu_request="500m", memory_request="512Mi")],
            metadata=ConversionMetadata("test-format", "/test/path", "2026-01-01T00:00:00Z"),
        )
        output = render_study_yaml(ir)
        assert "study:" in output
        assert "name: test" in output
        assert "web" in output
        # Should be valid YAML
        parsed = yaml.safe_load(output)
        assert parsed["study"]["name"] == "test"

    def test_scale_events_rendered(self):
        ir = ScenarioIR(
            name="test",
            cluster=Cluster(node_pools=[NodePool(instance_types=["m5.large"])]),
            workloads=[Workload(
                name="app",
                initial_replicas=2,
                cpu_request="100m",
                memory_request="256Mi",
                labels={"app": "app"},
                scaling_timeline=[(1, 5), (3, 2)],
            )],
            metadata=ConversionMetadata("test", "/test", "2026-01-01T00:00:00Z"),
        )
        output = render_study_yaml(ir)
        assert "increase_to: 5" in output
        assert "reduce_by: 3" in output

    def test_affinity_rendered(self):
        ir = ScenarioIR(
            name="test",
            cluster=Cluster(node_pools=[NodePool(instance_types=["m5.large"])]),
            workloads=[Workload(
                name="app",
                cpu_request="100m",
                memory_request="256Mi",
                labels={"app": "app"},
                pod_anti_affinity={"label_key": "app", "topology_key": "kubernetes.io/hostname", "affinity_type": "required"},
            )],
            metadata=ConversionMetadata("test", "/test", "2026-01-01T00:00:00Z"),
        )
        output = render_study_yaml(ir)
        assert "pod_anti_affinity:" in output
        assert 'label_key: "app"' in output


# ---------------------------------------------------------------------------
# Integration: round-trip with real fixture
# ---------------------------------------------------------------------------

class TestRoundTrip:
    @pytest.mark.skipif(not FIXTURE_DIR.exists(), reason="Fixture not available")
    def test_grafana_fixture_round_trip(self):
        """Convert the real Grafana fixture and verify all deployments and resources match."""
        adapter = K8sImportAdapter()
        ir = adapter.convert(FIXTURE_DIR)

        # Verify deployment count matches
        deploy_files = [f for f in sorted((FIXTURE_DIR / "deployments").glob("*.yaml"))
                        if not f.name.endswith("-pdb.yaml")]
        assert len(ir.workloads) == len(deploy_files)

        # Verify each deployment's resources match source
        for w in ir.workloads:
            dep_file = FIXTURE_DIR / "deployments" / f"{w.name}.yaml"
            assert dep_file.exists(), f"Missing deployment file for {w.name}"
            with dep_file.open() as f:
                doc = yaml.safe_load(f)
            req = doc["spec"]["template"]["spec"]["containers"][0].get("resources", {}).get("requests", {})
            assert w.cpu_request == _normalize_cpu(req.get("cpu", "0"))
            assert w.memory_request == _normalize_memory(req.get("memory", "0"))

    @pytest.mark.skipif(not FIXTURE_DIR.exists(), reason="Fixture not available")
    def test_grafana_scaling_events_match_source(self):
        """Verify scaling events match steps.yml for a sample of deployments."""
        from kubesim.converter.k8s_import import _parse_steps_streaming

        adapter = K8sImportAdapter()
        ir = adapter.convert(FIXTURE_DIR)
        source_timelines = _parse_steps_streaming(FIXTURE_DIR / "steps.yml")

        workload_map = {w.name: w for w in ir.workloads}
        for name, timeline in source_timelines.items():
            assert name in workload_map, f"Deployment {name} in steps.yml but not in IR"
            assert workload_map[name].scaling_timeline == sorted(timeline)

    @pytest.mark.skipif(not GOLDEN_REF.exists(), reason="Golden reference not available")
    def test_output_matches_golden_structure(self):
        """Verify converted output has same workload names as golden reference."""
        adapter = K8sImportAdapter()
        ir = adapter.convert(FIXTURE_DIR)
        output = render_study_yaml(ir)
        parsed = yaml.safe_load(output)

        with GOLDEN_REF.open() as f:
            golden = yaml.safe_load(f)

        output_names = sorted(w["labels"]["app"] for w in parsed["study"]["workloads"])
        golden_names = sorted(w["labels"]["app"] for w in golden["study"]["workloads"])
        assert output_names == golden_names
