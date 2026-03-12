# KIND+KWOK+Karpenter Validation Pipeline Plan

**Date:** 2026-03-12
**Bead:** k8s-7kb

---

## Architecture

```
KubeSim Scenario YAML
        │
        ▼
┌──────────────────┐
│ kubesim translate │  (existing: translator.py)
│ scenario → K8s   │
│ manifests        │
└────────┬─────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌────────┐ ┌──────────────────────────┐
│Tier 1  │ │Tier 2: KIND+KWOK+Karpenter│
│KubeSim │ │                          │
│N seeds │ │ KIND cluster             │
│(fast)  │ │ + KWOK fake nodes        │
│        │ │ + Karpenter controller   │
│        │ │ + metrics collector      │
└───┬────┘ └────────────┬─────────────┘
    │                   │
    ▼                   ▼
┌────────────────────────────┐
│ kubesim compare            │  (existing: compare.py)
│ distribution vs point est  │
│ → fidelity scorecard       │
└────────────────────────────┘
```

---

## 1. Scenario Translation

### What KIND+KWOK+Karpenter needs

Real K8s manifests: Namespace, Deployments, HPAs, PDBs, Services, Karpenter
NodePool CRDs, EC2NodeClass CRDs.

### Current state of `kubesim translate`

`translator.py` already generates:
- ✅ Namespace
- ✅ Deployments (with resource requests/limits, GPU, topology spread, priority classes)
- ✅ HPAs (cpu, memory, custom metrics)
- ✅ PDBs (minAvailable, maxUnavailable)
- ✅ Services
- ✅ Karpenter NodePool CRDs (instance type requirements, consolidation policy)
- ✅ EC2NodeClass CRDs (AMI selector, subnet/SG selectors)
- ✅ Variant-specific config (deletion cost annotations, PDB overrides)

### What's missing

| Gap | Impact | Effort |
|-----|--------|--------|
| KWOK Node templates (for pre-seeded fake nodes) | HIGH — KWOK needs explicit Node objects with allocatable resources | 0.5d |
| Karpenter-in-KIND setup manifests (Karpenter CRDs + controller + KWOK provider) | HIGH — biggest blocker, see §5 | 2-3d |
| Traffic simulation (load generator for HPA triggers) | MEDIUM — HPAs won't scale without actual CPU load | 1d |
| PriorityClass definitions (low-priority, high-priority) | LOW — referenced by pods but not created | 0.5h |
| Batch Job manifests (translator only emits Deployments) | LOW — batch_job archetype should emit Job, not Deployment | 0.5d |

### Recommended first validation scenario

A 10-20 minute wall-clock scenario exercising:
1. **Initial provisioning** — 5 Deployments submitted, Karpenter provisions nodes
2. **Scale-up** — HPA triggers replica increase, Karpenter adds nodes
3. **Scale-down** — Load drops, HPA scales down, consolidation kicks in
4. **Batch job** — Short-lived Job completes, node becomes empty → WhenEmpty consolidation
5. **Consolidation** — Underutilized nodes drained and terminated

```yaml
study:
  name: validation-baseline
  runs: 1000  # Tier 1 only
  time_mode: wall_clock
  cluster:
    node_pools:
      - instance_types: [m5.xlarge, m5.2xlarge]
        min_nodes: 0
        max_nodes: 20
        karpenter:
          consolidation: {policy: WhenUnderutilized}
  workloads:
    - type: web_app
      count: 3
      replicas: {min: 2, max: 10}
      scaling: {type: hpa, metric: cpu, target: 70%}
    - type: batch_job
      count: 2
      replicas: {fixed: 1}
  duration: 20m
```

---

## 2. Metrics Collection from Real Cluster

### Required metrics

| Metric | Source | Sampling | Maps to SimResult field |
|--------|--------|----------|------------------------|
| Node count over time | `kubectl get nodes` | Every 30s | `node_count` |
| Pod count by phase | `kubectl get pods` | Every 30s | `running_pods`, `pending_pods` |
| Scheduling latency | PodScheduled condition timestamp − creationTimestamp | Per pod | `scheduling_latencies_ms` |
| Cost proxy | Node instance-type labels → pricing table | Every 30s | `total_cost_per_hour` |
| Consolidation events | Karpenter controller logs + K8s events (reason=Evicted) | Event-driven | `disruption_events` |
| Node lifecycle events | K8s events (NodeReady, NodeNotReady) | Event-driven | `node_count_over_time` |

### Collection approach

Poll from outside the cluster (no sidecar needed for Tier 2):

```
┌─────────────────────────┐
│ metrics-collector.py    │  (new script, runs on host)
│                         │
│ loop every 30s:         │
│   kubectl get nodes -o json → node snapshot
│   kubectl get pods -o json  → pod snapshot
│   kubectl get events        → disruption events
│   append to time_series[]   │
│                         │
│ on completion:          │
│   export to parquet     │
└─────────────────────────┘
```

30s interval matches KubeSim's `MetricsSnapshot` event cadence.

### Current state of `kwok.py`

`collect_metrics()` takes a single snapshot at the end. It captures:
- ✅ Node count, pod count, running/pending
- ✅ Scheduling latencies (from PodScheduled condition)
- ✅ Pod phase transitions

Missing:
- ❌ **Time-series collection** — only a single point-in-time snapshot
- ❌ **Periodic sampling** — no polling loop during the settle period
- ❌ **Karpenter-specific events** — no consolidation/provisioning event capture
- ❌ **Cost estimation** — no instance-type → price mapping (EKS runner has it, KWOK doesn't)

---

## 3. Comparison Methodology

### Distribution-aware comparison

KubeSim runs N seeds → distribution. KIND+KWOK runs 1-3 times → point estimates.

**Approach: sigma scoring**

For each metric M:
1. From Tier 1 (N runs): compute mean(M), std(M)
2. From Tier 2 (1-3 runs): compute observed value obs(M)
3. Sigma score: `z = (obs(M) - mean(M)) / std(M)`
4. Fidelity rating:
   - |z| < 1.0 → 🟢 GREEN (within 1σ)
   - 1.0 ≤ |z| < 2.0 → 🟡 YELLOW (within 2σ)
   - |z| ≥ 2.0 → 🔴 RED (outside 2σ, likely fidelity gap)

### Per-metric comparison

| Metric | Comparison type | Tolerance |
|--------|----------------|-----------|
| `node_count` (final) | Sigma score | 1σ |
| `total_cost_per_hour` (final) | Sigma score | 1σ |
| `pending_pods` (max) | Sigma score | 1σ |
| `scheduling_latency` (p50, p99) | Sigma score | 2σ (latency is noisy) |
| `disruption_events` (count) | Sigma score | 1σ |
| `node_count_over_time` | DTW distance on time series | Normalized < 0.15 |

### Current state of `compare.py`

`compute_deltas()` computes scalar percentage deltas between tier means. It:
- ✅ Loads parquet, computes per-metric means
- ✅ Flags divergence above threshold
- ✅ Generates HTML report with bar charts

Missing:
- ❌ **Distribution-aware mode** — treats Tier 1 as a single mean, not a distribution
- ❌ **Sigma scoring** — only does `(tier_N - baseline) / baseline`
- ❌ **Time-series comparison** — no DTW or shape-based comparison
- ❌ **Per-metric tolerance** — single global threshold

---

## 4. Fidelity Gap Identification

From `accuracy-audit.md`, the gaps most likely to cause Tier 1 vs Tier 2 divergence:

### Timing differences

| Sim behavior | Real behavior | Expected divergence |
|-------------|---------------|-------------------|
| Instant NodeReady | Node launch 60-90s (KIND) or instant (KWOK) | Scheduling latency lower in sim if KWOK |
| Instant pod binding | API server round-trip ~10-50ms | Negligible |
| No consolidation cooldown | Karpenter `consolidateAfter: 30s` default | Sim consolidates faster |
| No scheduling backoff | kube-scheduler uses backoff queues | Sim retries faster |

### Controller behavior differences

| Sim behavior | Real behavior | Expected divergence |
|-------------|---------------|-------------------|
| Provisioning batches by (labels, taints, GPU) | Karpenter runs full scheduler simulation per pod | Node count may differ |
| Consolidation: deletion only | Karpenter: deletion + replacement | Sim underestimates consolidation savings |
| Consolidation: resource-only reschedulability | Karpenter: full constraint check | Sim may consolidate nodes that real Karpenter won't |
| Score normalization missing | kube-scheduler normalizes to [0,100] | Pod placement may differ when multiple scorers active |
| Cost-only candidate sorting | Karpenter: multi-factor (priority, PDB, age, pod count) | Different nodes consolidated |

### Biggest blocker: Karpenter-in-KIND

Running real Karpenter in KIND requires a cloud provider. Options:

1. **KWOK provider for Karpenter** — A fake cloud provider that creates KWOK nodes instead of EC2 instances. This is the cleanest approach but requires writing a Karpenter CloudProvider implementation.
   - Effort: 2-3 days
   - Benefit: Full Karpenter controller logic (provisioning, consolidation, drift) runs for real

2. **Mock webhook** — Intercept Karpenter's EC2 API calls with a local mock that returns fake instance data and creates KWOK nodes.
   - Effort: 1-2 days
   - Benefit: Less code, but fragile to Karpenter version changes

3. **Skip Karpenter, use Cluster Autoscaler** — CA is simpler to run in KIND (no cloud provider needed with `--cloud-provider=clusterapi`).
   - Effort: 0.5 days
   - Benefit: Quick, but doesn't validate Karpenter-specific behavior

**Recommendation:** Option 1 (KWOK provider). Karpenter validation is the whole point.

---

## 5. Tooling Needed

### New scripts/tools

| Tool | Purpose | Effort |
|------|---------|--------|
| `scripts/kind-karpenter-setup.sh` | Stand up KIND + install KWOK + Karpenter with KWOK provider | 1d |
| `validation/kwok_provider.py` | Karpenter CloudProvider that creates KWOK nodes (or Go if needed) | 2-3d |
| `validation/metrics_collector.py` | Time-series metrics poller (30s interval, exports parquet) | 0.5d |
| `validation/load_generator.py` | CPU stress pods to trigger HPA scaling | 0.5d |

### Modifications to existing code

| File | Change | Effort |
|------|--------|--------|
| `translator.py` | Add KWOK Node template generation, Job manifests, PriorityClass | 0.5d |
| `kwok.py` | Add time-series collection loop, cost estimation, Karpenter event capture | 1d |
| `compare.py` | Add distribution-aware sigma scoring, time-series DTW, per-metric tolerance | 1d |
| `cli.py` | Add `validate-kind-karpenter` command that orchestrates full pipeline | 0.5d |

### Total effort estimate: 7-10 days

---

## 6. Step-by-Step Runbook

### Prerequisites

- `kind` v0.24+
- `kubectl` v1.31+
- `helm` v3.16+
- Python 3.11+ with `polars`, `pyyaml`, `pyarrow`

### Steps

```
1. TRANSLATE scenario
   kubesim translate scenarios/validation-baseline.yaml -o manifests/ -v baseline

2. RUN Tier 1 (KubeSim, N=1000)
   python -m kubesim run scenarios/validation-baseline.yaml \
     --seeds 1000 --output results/tier1.parquet

3. SETUP KIND+KWOK+Karpenter cluster
   ./scripts/kind-karpenter-setup.sh create

4. APPLY manifests
   kubectl apply -f manifests/

5. COLLECT metrics (runs for scenario duration, polls every 30s)
   python -m kubesim.validation.metrics_collector \
     --context kind-kubesim --duration 20m --output results/tier2.parquet

6. COMPARE tiers
   kubesim compare results/tier1.parquet results/tier2.parquet \
     --mode sigma --threshold 2.0 --output results/fidelity-report.html

7. CLEANUP
   ./scripts/kind-karpenter-setup.sh delete
```

### Success criteria

The pipeline is "good enough" when:

1. **Infrastructure works end-to-end** — Steps 1-7 complete without manual intervention
2. **Metrics are comparable** — Tier 2 exports the same schema as Tier 1
3. **Fidelity scorecard is meaningful** — At least 4 of 6 metrics are 🟢 GREEN (within 1σ)
4. **Known gaps are documented** — 🔴 RED metrics have explanations traceable to accuracy-audit.md gaps
5. **Reproducible** — Running Tier 2 twice on the same scenario produces results within 10% of each other

### What "good enough" does NOT require

- Perfect agreement (some divergence is expected and informative)
- Tier 3 (EKS) validation (that's a separate, more expensive effort)
- Spot interruption validation (stochastic, hard to reproduce in KIND)
- HPA validation (requires real CPU load generation, deferred to phase 2)
