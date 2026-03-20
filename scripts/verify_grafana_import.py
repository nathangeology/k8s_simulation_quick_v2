#!/usr/bin/env python3
"""Verify and fix grafana-import-01.yaml scaling steps against source steps.yml.

Parses both files, reports mismatches, and generates corrected YAML.
"""

import re
import sys
import yaml
from pathlib import Path
from collections import defaultdict

REPO = Path(__file__).resolve().parent.parent
SOURCE_DIR = REPO / "scenarios" / "needs_conversion" / "k8s_import_grafana_1752595626"
CONVERTED = REPO / "scenarios" / "grafana-import-01.yaml"
STEPS_FILE = SOURCE_DIR / "steps.yml"
DEPLOY_DIR = SOURCE_DIR / "deployments"
CONFIG_FILE = SOURCE_DIR / "config.yml"


def parse_steps(path: Path) -> dict[str, list[tuple[int, int]]]:
    """Parse steps.yml streaming, return {deployment: [(step, replicas), ...]}."""
    timelines: dict[str, list[tuple[int, int]]] = defaultdict(list)
    current_step = None
    for line in path.open():
        m = re.match(r'\s+name:\s+(\d+)', line)
        if m:
            current_step = int(m.group(1))
            continue
        m = re.match(r'\s+action_data:\s+name=([^,]+),replicas=(\d+)', line)
        if m and current_step is not None:
            timelines[m.group(1)].append((current_step, int(m.group(2))))
    return dict(timelines)


def parse_deployment_initial_replicas(deploy_dir: Path) -> dict[str, int]:
    """Read initial replicas from deployment YAML files."""
    replicas = {}
    for f in sorted(deploy_dir.glob("*.yaml")):
        if f.name.endswith("-pdb.yaml"):
            continue
        with f.open() as fh:
            doc = yaml.safe_load(fh)
        name = doc["metadata"]["name"]
        replicas[name] = doc["spec"].get("replicas", 1)
    return replicas


def parse_deployment_resources(deploy_dir: Path) -> dict[str, dict]:
    """Read cpu/memory requests from deployment YAML files."""
    resources = {}
    for f in sorted(deploy_dir.glob("*.yaml")):
        if f.name.endswith("-pdb.yaml"):
            continue
        with f.open() as fh:
            doc = yaml.safe_load(fh)
        name = doc["metadata"]["name"]
        container = doc["spec"]["template"]["spec"]["containers"][0]
        req = container.get("resources", {}).get("requests", {})
        resources[name] = {
            "cpu": req.get("cpu", "0"),
            "memory": req.get("memory", "0"),
        }
    return resources


def normalize_cpu(val: str) -> str:
    """Normalize CPU to millicores string."""
    val = str(val).strip()
    if val.endswith("m"):
        return val
    return f"{int(float(val) * 1000)}m"


def normalize_memory(val: str) -> str:
    """Normalize memory to Mi string."""
    val = str(val).strip()
    if val.endswith("Mi"):
        return val
    if val.endswith("Gi"):
        return f"{int(float(val[:-2]) * 1024)}Mi"
    return val


def timeline_to_scale_events(
    initial: int, steps_data: list[tuple[int, int]]
) -> tuple[list[dict], list[dict]]:
    """Convert absolute timeline to scale_up and scale_down events."""
    scale_up = []
    scale_down = []
    prev = initial
    for step, replicas in sorted(steps_data):
        at = f"{step}m"
        if replicas > prev:
            scale_up.append({"at": at, "increase_to": replicas})
        elif replicas < prev:
            scale_down.append({"at": at, "reduce_by": prev - replicas})
        prev = replicas
    return scale_up, scale_down


def generate_corrected_yaml(
    initial_replicas: dict[str, int],
    deploy_resources: dict[str, dict],
    steps_data: dict[str, list[tuple[int, int]]],
    config: dict,
) -> str:
    """Generate corrected scenario YAML."""
    lines = []
    lines.append("# Grafana production cluster scenario (imported from external simulator format)")
    lines.append("# Source: scenarios/needs_conversion/k8s_import_grafana_1752595626/")
    lines.append(f"# {len(initial_replicas)} deployments, 60-minute scaling timeline, ~5500 peak pods")
    lines.append("")
    lines.append("study:")
    lines.append("  name: grafana-import-01")
    lines.append("  runs: 50")
    lines.append("  time_mode: wall_clock")
    lines.append("")
    lines.append("  cluster:")
    lines.append("    node_pools:")
    # Use EC2 m5 family based on source config (m5.large) with larger sizes for bin-packing
    lines.append("      - instance_types: [m5.large, m5.xlarge, m5.2xlarge, m5.4xlarge, m5.8xlarge, m5.12xlarge, m5.16xlarge, m5.24xlarge]")
    lines.append("        min_nodes: 0")
    lines.append("        max_nodes: 1000")
    lines.append("        karpenter:")
    lines.append("          consolidation: {policy: WhenUnderutilized}")
    lines.append("    daemonsets:")
    lines.append("      - name: kube-proxy")
    lines.append('        cpu_request: "100m"')
    lines.append('        memory_request: "256Mi"')
    lines.append("      - name: node-agent")
    lines.append('        cpu_request: "50m"')
    lines.append('        memory_request: "256Mi"')
    lines.append("    delays:")
    lines.append('      node_startup: "30s"')
    lines.append('      node_startup_jitter: "10s"')
    lines.append('      node_shutdown: "5s"')
    lines.append('      provisioner_batch: "10s"')
    lines.append('      provisioner_batch_jitter: "5s"')
    lines.append('      pod_startup: "2s"')
    lines.append("")
    lines.append("  workloads:")

    # Sort deployments alphabetically for deterministic output
    for name in sorted(initial_replicas.keys()):
        init = initial_replicas[name]
        res = deploy_resources.get(name, {"cpu": "0", "memory": "0"})
        cpu = normalize_cpu(res["cpu"])
        mem = normalize_memory(res["memory"])

        lines.append(f"    # {name}")
        lines.append("    - type: web_app")
        lines.append("      count: 1")
        lines.append(f"      replicas: {{fixed: {init}}}")
        lines.append(f'      cpu_request: {{dist: uniform, min: "{cpu}", max: "{cpu}"}}')
        lines.append(f'      memory_request: {{dist: uniform, min: "{mem}", max: "{mem}"}}')
        lines.append(f"      labels: {{app: {name}}}")

        if name in steps_data:
            scale_up, scale_down = timeline_to_scale_events(init, steps_data[name])
            if scale_up:
                lines.append("      scale_up:")
                for ev in scale_up:
                    lines.append(f'        - {{at: "{ev["at"]}", increase_to: {ev["increase_to"]}}}')
            if scale_down:
                lines.append("      scale_down:")
                for ev in scale_down:
                    lines.append(f'        - {{at: "{ev["at"]}", reduce_by: {ev["reduce_by"]}}}')

    return "\n".join(lines) + "\n"


def compare_events(expected: list[dict], actual: list[dict], kind: str) -> list[str]:
    """Compare scale event lists, return list of mismatch descriptions."""
    diffs = []
    if len(expected) != len(actual):
        diffs.append(f"  {kind}: expected {len(expected)} events, got {len(actual)}")
    for i, (e, a) in enumerate(zip(expected, actual)):
        if e != a:
            diffs.append(f"  {kind}[{i}]: expected {e}, got {a}")
    if len(expected) > len(actual):
        for i in range(len(actual), len(expected)):
            diffs.append(f"  {kind}[{i}]: MISSING expected {expected[i]}")
    elif len(actual) > len(expected):
        for i in range(len(expected), len(actual)):
            diffs.append(f"  {kind}[{i}]: EXTRA got {actual[i]}")
    return diffs


def parse_converted(path: Path) -> dict[str, dict]:
    """Parse converted YAML workloads."""
    with path.open() as f:
        doc = yaml.safe_load(f)
    workloads = {}
    for w in doc["study"]["workloads"]:
        name = w["labels"]["app"]
        cpu_val = w.get("cpu_request", {})
        mem_val = w.get("memory_request", {})
        cpu = cpu_val.get("min", "0") if isinstance(cpu_val, dict) else str(cpu_val)
        mem = mem_val.get("min", "0") if isinstance(mem_val, dict) else str(mem_val)
        workloads[name] = {
            "replicas": w["replicas"]["fixed"],
            "scale_up": w.get("scale_up", []),
            "scale_down": w.get("scale_down", []),
            "cpu": cpu,
            "memory": mem,
        }
    return workloads


def main():
    fix = "--fix" in sys.argv

    print("=== Grafana Import Verification ===\n")

    print("Parsing steps.yml...")
    steps_data = parse_steps(STEPS_FILE)
    print(f"  Found {len(steps_data)} deployments with scaling actions")

    print("Parsing deployment YAMLs...")
    initial_replicas = parse_deployment_initial_replicas(DEPLOY_DIR)
    deploy_resources = parse_deployment_resources(DEPLOY_DIR)
    print(f"  Found {len(initial_replicas)} deployments")

    print("Parsing converted YAML...")
    converted = parse_converted(CONVERTED)
    print(f"  Found {len(converted)} workloads\n")

    # Node pool check
    print("--- Node Pool Check ---")
    with CONVERTED.open() as f:
        doc = yaml.safe_load(f)
    catalog = doc["study"].get("catalog_provider", "(not set, defaults to ec2)")
    pools = doc["study"]["cluster"]["node_pools"]
    print(f"  catalog_provider: {catalog}")
    for i, pool in enumerate(pools):
        types = pool["instance_types"]
        print(f"  pool[{i}] instance_types: {types[:3]}... ({len(types)} total)")
    with CONFIG_FILE.open() as f:
        config = yaml.safe_load(f)
    src_node = config["simulator"]["clusters"][0]["KubernetesCluster"]["node_type"]
    src_pool = config["simulator"]["instance_pool_size"]
    print(f"  Source: node_type={src_node}, instance_pool_size={src_pool}")
    if "catalog_provider" in doc["study"]:
        print(f"  ⚠ MISMATCH: catalog_provider={catalog}, should use EC2 (remove catalog_provider)")
    print()

    # Scaling comparison
    print("--- Scaling Timeline Comparison ---")
    mismatches = 0
    for name in sorted(set(list(steps_data.keys()) + list(converted.keys()))):
        if name not in converted:
            print(f"  ✗ {name}: in steps.yml but MISSING from converted YAML")
            mismatches += 1
            continue
        if name not in steps_data:
            c = converted[name]
            if c["scale_up"] or c["scale_down"]:
                print(f"  ✗ {name}: no scaling in steps.yml but converted has events")
                mismatches += 1
            continue

        init = initial_replicas.get(name, 1)
        exp_up, exp_down = timeline_to_scale_events(init, steps_data[name])
        c = converted[name]

        diffs = []
        if c["replicas"] != init:
            diffs.append(f"  replicas: expected {init}, got {c['replicas']}")
        diffs.extend(compare_events(exp_up, c["scale_up"], "scale_up"))
        diffs.extend(compare_events(exp_down, c["scale_down"], "scale_down"))

        if diffs:
            print(f"  ✗ {name}:")
            for d in diffs[:5]:
                print(f"    {d}")
            if len(diffs) > 5:
                print(f"    ... and {len(diffs) - 5} more")
            mismatches += 1

    print(f"\n  Total: {len(converted)} workloads, {mismatches} with mismatches")

    if fix:
        print("\n--- Generating corrected YAML ---")
        corrected = generate_corrected_yaml(
            initial_replicas, deploy_resources, steps_data, config
        )
        CONVERTED.write_text(corrected)
        print(f"  ✓ Wrote corrected YAML to {CONVERTED}")
        print("  Re-running verification...")
        # Re-verify
        converted2 = parse_converted(CONVERTED)
        m2 = 0
        for name in sorted(set(list(steps_data.keys()) + list(converted2.keys()))):
            if name not in converted2 or name not in steps_data:
                continue
            init = initial_replicas.get(name, 1)
            exp_up, exp_down = timeline_to_scale_events(init, steps_data[name])
            c = converted2[name]
            diffs = []
            if c["replicas"] != init:
                diffs.append("replicas")
            diffs.extend(compare_events(exp_up, c["scale_up"], "scale_up"))
            diffs.extend(compare_events(exp_down, c["scale_down"], "scale_down"))
            if diffs:
                m2 += 1
        if m2 == 0:
            print("  ✓ All scaling timelines now match!")
        else:
            print(f"  ✗ Still {m2} mismatches after correction")
        return m2
    elif mismatches > 0:
        print(f"\n✗ {mismatches} mismatches found. Run with --fix to generate corrected YAML.")
    else:
        print("\n✓ All scaling timelines match!")

    return mismatches


if __name__ == "__main__":
    sys.exit(0 if main() == 0 else 1)
