# kwok Verification Plan: ConsolidateWhen Simulator Results

**Bead:** k8s-szsq
**PR ref:** https://github.com/kubernetes-sigs/karpenter/pull/2893
**Simulator results:** `results/consolidate-when/benchmark-tradeoff/`

---

## 1. Objective

Verify that the kubesim simulator's ConsolidateWhen tradeoff predictions match
real Karpenter behavior by replaying the benchmark-control workload against a
kwok cluster running Karpenter from PR #2893.

### Key Simulator Predictions to Verify

| Prediction | Simulator Value | Source |
|------------|----------------|--------|
| WhenEmptyOrUnderutilized is most expensive AND most disruptive | 27 evictions | benchmark-tradeoff |
| CostJustified 0.25–2.0 saves ~0.55% with ~10 evictions | — | benchmark-tradeoff |
| CostJustified 5.0 is inflection point | 4 evictions, 0.10% savings | benchmark-tradeoff |
| WhenEmpty has 0 disruptions | 0 evictions | benchmark-tradeoff |
| Policy ordering: CostJustified-1.0 < WhenEmpty on cost | — | benchmark-tradeoff |

---

## 2. Test Environment

### 2.1 Cluster Stack

```
KIND cluster (1 control-plane + 1 worker)
├── KWOK controller (latest release)
├── Karpenter from PR #2893 branch (built from source)
├── kube-state-metrics (for pod/node metrics)
└── Prometheus (optional, for karpenter_consolidation_decision_ratio histogram)
```

### 2.2 Building Karpenter from PR #2893

The PR adds `ConsolidateWhen` + `DecisionRatioThreshold` to the NodePool spec.
Build from the PR branch instead of a release tag:

```bash
WORK_DIR="$(pwd)/validation/.work"
mkdir -p "$WORK_DIR"

# Clone and checkout PR branch
if [ ! -d "$WORK_DIR/karpenter" ]; then
    git clone https://github.com/kubernetes-sigs/karpenter.git "$WORK_DIR/karpenter"
fi
cd "$WORK_DIR/karpenter"
git fetch origin pull/2893/head:pr-2893
git checkout pr-2893

# Build with KWOK provider and load into KIND
export KIND_CLUSTER_NAME=kubesim-val
make build-with-kind
kubectl apply -f kwok/charts/crds
helm upgrade --install karpenter kwok/charts --namespace kube-system --skip-crds \
    --set controller.image.repository=kind.local/karpenter \
    --set controller.image.tag=latest \
    --set serviceMonitor.enabled=true \
    --wait --timeout 300s
```

### 2.3 Cluster Setup Script

Extend `validation/setup-cluster.sh` with a `KARPENTER_REF` variable:

```bash
KARPENTER_REF=${KARPENTER_REF:-v1.1.0}  # default to release tag
# For PR #2893:
# KARPENTER_REF=pull/2893/head ./validation/setup-cluster.sh
```

When `KARPENTER_REF` starts with `pull/`, fetch the PR branch instead of
checking out a tag. Everything else (KWOK, kube-state-metrics) stays the same.

---

## 3. Verification Matrix

### 3.1 Variants

Run the same 10 variants as the simulator analysis. Each variant gets its own
NodePool CRD applied before the workload run:

| # | Variant | NodePool Spec |
|---|---------|---------------|
| 1 | when-empty | `consolidationPolicy: WhenEmpty` |
| 2 | when-underutilized | `consolidationPolicy: WhenEmptyOrUnderutilized` |
| 3 | cost-justified-0.25 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "0.25"` |
| 4 | cost-justified-0.50 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "0.50"` |
| 5 | cost-justified-0.75 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "0.75"` |
| 6 | cost-justified-1.00 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "1.00"` |
| 7 | cost-justified-1.50 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "1.50"` |
| 8 | cost-justified-2.00 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "2.00"` |
| 9 | cost-justified-3.00 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "3.00"` |
| 10 | cost-justified-5.00 | `consolidateWhen: WhenCostJustifiesDisruption`, `decisionRatioThreshold: "5.00"` |

### 3.2 NodePool CRD Templates

For variants 1–2 (existing policies):

```yaml
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: default
spec:
  template:
    spec:
      requirements:
        - key: kubernetes.io/arch
          operator: In
          values: ["amd64"]
        - key: node.kubernetes.io/instance-type
          operator: In
          values: ["m-4x-amd64-linux", "m-8x-amd64-linux"]
      nodeClassRef:
        name: default
        kind: KWOKNodeClass
        group: karpenter.kwok.sh
  limits:
    cpu: "1000"
  disruption:
    consolidationPolicy: ${POLICY}       # WhenEmpty | WhenEmptyOrUnderutilized
    consolidateAfter: 30s
```

For variants 3–10 (PR #2893 new field):

```yaml
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: default
spec:
  template:
    spec:
      requirements:
        - key: kubernetes.io/arch
          operator: In
          values: ["amd64"]
        - key: node.kubernetes.io/instance-type
          operator: In
          values: ["m-4x-amd64-linux", "m-8x-amd64-linux"]
      nodeClassRef:
        name: default
        kind: KWOKNodeClass
        group: karpenter.kwok.sh
  limits:
    cpu: "1000"
  disruption:
    consolidateWhen: WhenCostJustifiesDisruption
    decisionRatioThreshold: "${THRESHOLD}"
    consolidateAfter: 30s
```

**Note:** The exact field names (`consolidateWhen`, `decisionRatioThreshold`)
must be confirmed against the PR #2893 CRD schema. Check
`kwok/charts/crds/karpenter.sh_nodepools.yaml` in the PR branch.

### 3.3 Workload Manifests

Match the benchmark-control scenario exactly:

```yaml
# deployment-a.yaml — CPU-bound workload
apiVersion: apps/v1
kind: Deployment
metadata:
  name: workload-a
  labels:
    kubesim-scenario: consolidate-when-verify
spec:
  replicas: 1
  selector:
    matchLabels: {app: workload-a}
  template:
    metadata:
      labels: {app: workload-a}
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 950m
            memory: 3.5Gi
---
# deployment-b.yaml — Memory-bound workload
apiVersion: apps/v1
kind: Deployment
metadata:
  name: workload-b
  labels:
    kubesim-scenario: consolidate-when-verify
spec:
  replicas: 1
  selector:
    matchLabels: {app: workload-b}
  template:
    metadata:
      labels: {app: workload-b}
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 950m
            memory: 6.5Gi
```

### 3.4 Scale Sequence

Replicate the simulator's scale-out/scale-in pattern:

| Time | Action | Replicas (each deployment) |
|------|--------|---------------------------|
| t=0 | Deploy | 1 |
| t=10s | Scale up | 500 |
| t=15m | Scale down | 350 |
| t=25m | Scale down | 10 |
| t=35m | Collect final metrics, end run | — |

---

## 4. Metrics Collection

### 4.1 Metrics to Capture

Reuse `validation/collect-metrics.sh` (30s interval) plus Karpenter-specific metrics:

| Metric | Source | Collection Method |
|--------|--------|-------------------|
| `node_count` | kubectl | collect-metrics.sh (existing) |
| `pod_count` | kubectl | collect-metrics.sh (existing) |
| `pending_count` | kubectl | collect-metrics.sh (existing) |
| `total_cost_per_hour` | node labels | collect-metrics.sh (existing) |
| `pods_evicted` | Karpenter logs | grep `disrupting` in controller logs |
| `disruption_count` | Karpenter logs | count consolidation actions |
| `consolidation_decision_ratio` | Prometheus | `karpenter_consolidation_decision_ratio` histogram (PR #2893) |
| `consolidation_latency` | timestamps | time from scale-down to stable node count |

### 4.2 Karpenter Log Scraping

The PR #2893 Karpenter emits structured logs for consolidation decisions. Capture:

```bash
# After each run, extract consolidation events
kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter --since=40m \
  | grep -E '(disrupting|consolidat|decision.ratio)' \
  > "$RESULTS_DIR/${VARIANT}/karpenter-consolidation.log"
```

### 4.3 Eviction Counting

```bash
# Count pod disruptions from Karpenter events
kubectl get events --field-selector reason=Evicted -o json \
  | jq '[.items[] | .involvedObject.name] | length'

# Or from Karpenter logs (more reliable with kwok):
grep -c 'disrupting node' "$RESULTS_DIR/${VARIANT}/karpenter-consolidation.log"
```

---

## 5. Automation Scripts

### 5.1 `scripts/run-kwok-consolidate-verify.sh`

Main orchestrator. Runs all 10 variants sequentially:

```
for each variant in VARIANTS:
    1. Apply NodePool CRD for this variant
    2. Wait for Karpenter to reconcile (10s)
    3. Deploy workloads (replicas=1)
    4. Start metrics collection (background)
    5. Execute scale sequence (scale up → wait → scale down → wait → scale down → wait)
    6. Collect final metrics + Karpenter logs
    7. Delete workloads, delete all kwok nodes
    8. Reset NodePool to clean state
    9. Save results to results/kwok-verify/${variant}/
```

Estimated runtime per variant: ~40 minutes (35m scenario + 5m setup/teardown).
Total for 10 variants: ~7 hours.

### 5.2 `scripts/compare-kwok-vs-simulator.py`

Comparison script that loads both result sets and produces a report:

```
Inputs:
  - results/consolidate-when/benchmark-tradeoff/  (simulator)
  - results/kwok-verify/                           (kwok)

Outputs:
  - results/kwok-verify/comparison-report.md
  - results/kwok-verify/comparison-plots/
      - node_count_overlay.png      (sim vs kwok per variant)
      - cost_comparison.png         (bar chart)
      - disruption_comparison.png   (bar chart)
      - pareto_overlay.png          (sim frontier vs kwok points)
```

Comparison logic per variant:
1. Interpolate both timeseries to common time points (30s intervals)
2. Compute per-metric deltas: `abs(sim - kwok) / max(sim, kwok)`
3. Flag variants where delta exceeds tolerance threshold

---

## 6. Success Criteria & Tolerances

### 6.1 Per-Metric Tolerances

| Metric | Tolerance | Rationale |
|--------|-----------|-----------|
| `node_count` (time-weighted) | ±15% | kwok timing jitter, batch window differences |
| `pods_evicted` | ±3 absolute | Karpenter may batch evictions differently |
| `total_cost` (cumulative) | ±10% | Node type selection may differ slightly |
| `consolidation_latency` | ±60s | Real scheduler has variable reconcile loops |
| `peak_node_count` | ±5% | Should be nearly identical (same workload) |

### 6.2 Structural Success Criteria

These must hold regardless of absolute values:

1. **Policy ordering preserved:** If simulator says variant A costs less than
   variant B, kwok must agree (rank correlation ≥ 0.85 Spearman).

2. **Disruption monotonicity:** Higher `decisionRatioThreshold` → fewer or equal
   evictions in kwok (same as simulator).

3. **WhenEmpty zero-disruption:** kwok WhenEmpty variant must show 0 pod evictions
   (matching simulator prediction).

4. **Inflection point agreement:** The threshold at which evictions drop sharply
   should be within ±1 step of the simulator's inflection (e.g., if simulator
   says 5.0, kwok should show the drop between 3.0 and 5.0).

5. **WhenEmptyOrUnderutilized most disruptive:** kwok must confirm this variant
   has the highest eviction count.

### 6.3 Failure Handling

| Outcome | Action |
|---------|--------|
| All criteria pass | Simulator validated — document in results |
| Structural criteria pass, absolute values outside tolerance | Acceptable — document divergence, tune simulator parameters |
| Structural criteria fail (ordering wrong) | Simulator bug — file bead, investigate consolidation model |
| kwok cluster unstable / Karpenter crashes | Infrastructure issue — retry, file upstream issue if persistent |

---

## 7. Known Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| PR #2893 CRD schema differs from simulator assumptions | NodePool apply fails | Check CRD schema first; adapt templates |
| KWOK node startup timing differs from simulator delays | Timing-sensitive metrics diverge | Use time-weighted metrics; increase tolerance |
| Karpenter consolidation loop timing is non-deterministic | Run-to-run variance | Run each variant 3× and average |
| PR #2893 branch may not build cleanly with KWOK provider | Build failure | Fall back to cherry-picking consolidation commits onto a known-good KWOK branch |
| kwok nodes don't report realistic resource usage | Consolidation decisions based on requests only | Acceptable — simulator also uses requests, not utilization |

---

## 8. Implementation Order

1. **Verify PR #2893 builds** — Clone, checkout, build with KWOK provider
2. **Confirm CRD schema** — Check exact field names for `consolidateWhen` / `decisionRatioThreshold`
3. **Create NodePool templates** — One per variant, parameterized
4. **Create workload manifests** — `deployment-a.yaml`, `deployment-b.yaml`
5. **Write orchestrator script** — `scripts/run-kwok-consolidate-verify.sh`
6. **Write comparison script** — `scripts/compare-kwok-vs-simulator.py`
7. **Run single variant smoke test** — `when-empty` (simplest, fastest)
8. **Run full matrix** — All 10 variants
9. **Generate comparison report** — `results/kwok-verify/comparison-report.md`

---

## 9. Directory Structure

```
k8s/
├── validation/
│   ├── setup-cluster.sh          # Extended with KARPENTER_REF support
│   ├── collect-metrics.sh        # Existing (reuse as-is)
│   └── nodepools/
│       ├── when-empty.yaml
│       ├── when-underutilized.yaml
│       ├── cost-justified-0.25.yaml
│       ├── ...
│       └── cost-justified-5.00.yaml
├── scripts/
│   ├── run-kwok-consolidate-verify.sh    # Orchestrator
│   └── compare-kwok-vs-simulator.py      # Comparison + plots
├── results/
│   └── kwok-verify/
│       ├── when-empty/
│       │   ├── timeseries.json
│       │   └── karpenter-consolidation.log
│       ├── when-underutilized/
│       │   └── ...
│       ├── cost-justified-0.25/
│       │   └── ...
│       ├── ...
│       ├── comparison-report.md
│       └── comparison-plots/
└── docs/
    └── kwok-verification-plan.md         # This document
```
