"""Render a ScenarioIR to native study YAML (deterministic, sorted output)."""

from __future__ import annotations

from .base import ScenarioIR, Workload


def _scale_events(workload: Workload) -> tuple[list[dict], list[dict]]:
    """Convert absolute scaling timeline to scale_up / scale_down events."""
    scale_up: list[dict] = []
    scale_down: list[dict] = []
    prev = workload.initial_replicas
    for step, replicas in workload.scaling_timeline:
        at = f"{step}m"
        if replicas > prev:
            scale_up.append({"at": at, "increase_to": replicas})
        elif replicas < prev:
            scale_down.append({"at": at, "reduce_by": prev - replicas})
        prev = replicas
    return scale_up, scale_down


def render_study_yaml(ir: ScenarioIR) -> str:
    """Render *ir* to a deterministic study YAML string."""
    lines: list[str] = []

    # Header comment
    lines.append(f"# Converted from {ir.metadata.source_format} format")
    lines.append(f"# Source: {ir.metadata.source_path}")
    lines.append(f"# {len(ir.workloads)} deployments, converted {ir.metadata.converted_at}")
    lines.append("")

    lines.append("study:")
    lines.append(f"  name: {ir.name}")
    lines.append(f"  runs: {ir.runs}")
    lines.append(f"  time_mode: {ir.time_mode}")
    lines.append("")

    # Cluster
    lines.append("  cluster:")
    lines.append("    node_pools:")
    for pool in ir.cluster.node_pools:
        types_str = ", ".join(pool.instance_types)
        lines.append(f"      - instance_types: [{types_str}]")
        lines.append(f"        min_nodes: {pool.min_nodes}")
        lines.append(f"        max_nodes: {pool.max_nodes}")
        lines.append("        karpenter:")
        lines.append(f"          consolidation: {{policy: {pool.consolidation_policy}}}")
    if ir.cluster.daemonsets:
        lines.append("    daemonsets:")
        for ds in ir.cluster.daemonsets:
            lines.append(f"      - name: {ds.name}")
            lines.append(f'        cpu_request: "{ds.cpu_request}"')
            lines.append(f'        memory_request: "{ds.memory_request}"')
    d = ir.cluster.delays
    lines.append("    delays:")
    lines.append(f'      node_startup: "{d.node_startup}"')
    lines.append(f'      node_startup_jitter: "{d.node_startup_jitter}"')
    lines.append(f'      node_shutdown: "{d.node_shutdown}"')
    lines.append(f'      provisioner_batch: "{d.provisioner_batch}"')
    lines.append(f'      provisioner_batch_jitter: "{d.provisioner_batch_jitter}"')
    lines.append(f'      pod_startup: "{d.pod_startup}"')
    lines.append("")

    # Workloads
    lines.append("  workloads:")
    for w in ir.workloads:
        lines.append(f"    # {w.name}")
        lines.append(f"    - type: {w.workload_type}")
        lines.append("      count: 1")
        lines.append(f"      replicas: {{fixed: {w.initial_replicas}}}")
        lines.append(f'      cpu_request: {{dist: uniform, min: "{w.cpu_request}", max: "{w.cpu_request}"}}')
        lines.append(f'      memory_request: {{dist: uniform, min: "{w.memory_request}", max: "{w.memory_request}"}}')

        # Labels
        label_parts = ", ".join(f"{k}: {v}" for k, v in sorted(w.labels.items()))
        lines.append(f"      labels: {{{label_parts}}}")

        # Affinity
        if w.pod_anti_affinity:
            aa = w.pod_anti_affinity
            lines.append("      pod_anti_affinity:")
            lines.append(f'        label_key: "{aa["label_key"]}"')
            lines.append(f'        topology_key: "{aa["topology_key"]}"')
            lines.append(f'        affinity_type: "{aa["affinity_type"]}"')

        # Topology spread
        if w.topology_spread:
            ts = w.topology_spread
            lines.append(f"      topology_spread: {{max_skew: {ts['max_skew']}, topology_key: {ts['topology_key']}}}")

        # Scaling events
        scale_up, scale_down = _scale_events(w)
        if scale_up:
            lines.append("      scale_up:")
            for ev in scale_up:
                lines.append(f'        - {{at: "{ev["at"]}", increase_to: {ev["increase_to"]}}}')
        if scale_down:
            lines.append("      scale_down:")
            for ev in scale_down:
                lines.append(f'        - {{at: "{ev["at"]}", reduce_by: {ev["reduce_by"]}}}')

    # Variants
    if ir.variants:
        lines.append("")
        lines.append("  variants:")
        for v in ir.variants:
            lines.append(f"    - name: {v.name}")
            if v.scheduler:
                sched_parts = ", ".join(f"{k}: {val}" for k, val in sorted(v.scheduler.items()))
                lines.append(f"      scheduler: {{{sched_parts}}}")

    return "\n".join(lines) + "\n"
