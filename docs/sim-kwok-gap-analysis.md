# Simulator vs KWOK Gap Analysis

**Bead:** k8s-f6jd
**Date:** 2026-03-23
**Data source:** `results/kwok-verify/` (fast-mode runs, 50 replicas per deployment, ~2min window)

---

## Executive Summary

The KWOK verification run revealed that **no consolidation occurred in any variant**.
All Karpenter consolidation logs are 0 bytes, all variants show 0 evictions, and pods
remained in `Pending` state throughout every run. The data does not validate or
invalidate the simulator's consolidation model — it reveals that the KWOK test
environment failed to produce schedulable pods, making consolidation impossible.

The simulator's **policy ordering** (CostJustified < WhenEmpty on node count) is
partially visible in the provisioning behavior, but the consolidation predictions
(evictions, node reclamation) cannot be compared because consolidation never triggered.

---

## Gap 1: Node Count Divergence

### Observed

| Variant | Sim Prediction | KWOK Actual | Delta |
|---------|---------------|-------------|-------|
| WhenEmpty | 6 nodes | 14 nodes | +8 (sim under-predicts) |
| WhenUnderutilized | 6 nodes | 14 nodes | +8 |
| CostJustified (all thresholds) | 3 nodes | 1 node | −2 (sim over-predicts) |

### Analysis

The divergence splits into two distinct failure modes:

**WhenEmpty / WhenUnderutilized (14 nodes):** Karpenter provisioned 14 nodes
immediately upon seeing 100 pending pods (2 deployments × 50 replicas). The
simulator predicts 6 nodes because it models bin-packing of pods onto
`m-4x-amd64-linux` and `m-8x-amd64-linux` instance types. The KWOK cluster
provisioned more nodes because:
- KWOK nodes may not have reported accurate allocatable resources, causing
  Karpenter to provision additional nodes when pods remained unschedulable
- The simulator assumes perfect bin-packing; real Karpenter uses a greedy
  provisioning algorithm that may over-provision when pod scheduling fails

**CostJustified variants (1 node):** Karpenter never provisioned beyond the
initial control-plane/worker node. The `consolidateWhen: WhenCostJustifiesDisruption`
field from PR #2893 may not have been recognized by the Karpenter build, causing
the NodePool to be applied without a valid disruption policy. Without a recognized
consolidation policy, Karpenter may have defaulted to a no-op or errored silently,
preventing provisioning entirely.

### Root Cause

1. **KWOK node resource reporting:** KWOK nodes likely did not report `Ready`
   condition or accurate `allocatable` resources, preventing the scheduler from
   placing pods. Evidence: all pods remained `Pending` throughout every run
   (pending count = pod count at every timeseries sample).

2. **CRD field recognition:** The `consolidateWhen` and `decisionRatioThreshold`
   fields from PR #2893 may not have been present in the Karpenter build used.
   The CostJustified variants all show identical behavior (1 node, no provisioning),
   suggesting the NodePool spec was rejected or ignored.

### Simulator Calibration

No calibration possible from this data. The node count divergence is caused by
infrastructure issues (KWOK node readiness), not simulator model error.

---

## Gap 2: Eviction Mismatch (0 Everywhere)

### Observed

| Variant | Sim Prediction | KWOK Actual |
|---------|---------------|-------------|
| WhenEmpty | 0 evictions | 0 evictions |
| WhenUnderutilized | 9.6 evictions | 0 evictions |
| CostJustified-0.25 | ~27 evictions | 0 evictions |
| CostJustified-1.00 | ~10 evictions | 0 evictions |
| CostJustified-5.00 | ~4 evictions | 0 evictions |

### Analysis

All `karpenter-consolidation.log` files are **0 bytes** — Karpenter emitted no
consolidation-related log lines (`disrupting`, `consolidat`, `decision.ratio`)
during any variant run.

Consolidation requires:
1. Pods successfully scheduled onto nodes (not Pending)
2. Scale-down creating underutilized or empty nodes
3. `consolidateAfter: 30s` timer to expire
4. Karpenter's consolidation loop to evaluate and act

Since pods never left `Pending` state, condition (1) was never met. Nodes were
never loaded, so they were never "underutilized" in the Karpenter sense — they
were empty but had pending pods that couldn't schedule, which may prevent
WhenEmpty consolidation as well (Karpenter won't remove a node if pods are
targeting it).

### Does KWOK Karpenter Actually Evict?

This remains unverified. KWOK Karpenter *should* evict pods during consolidation
(it uses the same controller logic), but the drain-and-cordon flow may behave
differently with KWOK's simulated kubelet. The 0-eviction result here is caused
by consolidation never triggering, not by a drain/eviction mechanism difference.

### Simulator Calibration

The simulator's WhenEmpty = 0 evictions prediction is trivially confirmed (both
show 0), but this is not meaningful validation since consolidation didn't run.
No calibration adjustments warranted from this data.

---

## Gap 3: WhenEmptyOrUnderutilized ≡ WhenEmpty in KWOK

### Observed

| Metric | WhenEmpty | WhenUnderutilized | Identical? |
|--------|-----------|-------------------|------------|
| Final nodes | 14 | 14 | ✅ |
| Final pods | 4 | 4 | ✅ |
| Evictions | 0 | 0 | ✅ |
| Timeseries shape | 1→14→14→14 | 1→14→14→14 | ✅ |

The simulator predicts WhenEmptyOrUnderutilized should be the **most aggressive**
policy (2 nodes, 27 evictions), but KWOK shows it behaving identically to WhenEmpty.

### Analysis

Since pods never scheduled, both policies saw the same state: nodes with 0
running pods but pending pods in the queue. Neither policy's consolidation
logic could differentiate because the precondition (pods running on nodes)
was never met.

The identical behavior is an artifact of the broken scheduling, not evidence
that the policies are equivalent. In a working cluster with scheduled pods:
- WhenEmpty would only consolidate nodes with 0 running pods
- WhenEmptyOrUnderutilized would also consolidate nodes below utilization threshold

### Simulator Calibration

No calibration possible. The policy differentiation requires working pod scheduling.

---

## Gap 4: Scale Mismatch

### Observed

| Parameter | Simulator | KWOK Fast Mode |
|-----------|-----------|----------------|
| Replicas per deployment | 500 | 50 |
| Scale factor | 1× | 0.1× |
| Total pods at peak | 1000 | 100 |
| Run duration | 35 min | ~2 min |
| consolidateAfter | 30s | 30s |

### Analysis

The 10× scale reduction changes consolidation dynamics:
- **Bin-packing efficiency:** 100 pods across 2 instance types produces different
  packing ratios than 1000 pods. Fewer pods means fewer partially-filled nodes,
  which reduces consolidation opportunities.
- **Timing:** The fast-mode 2-minute window with 30s consolidateAfter leaves only
  ~1-2 consolidation cycles after each scale-down event. The full 35-minute
  sequence allows ~20+ cycles, giving Karpenter time to iteratively consolidate.
- **Node count:** At 50 replicas with ~1 CPU per pod, the expected node count is
  much lower than at 500 replicas, making the absolute comparison meaningless.

### Impact on Comparison

Even if scheduling worked, the fast-mode results would not be directly comparable
to simulator predictions because:
1. Different absolute scale → different bin-packing → different node counts
2. Compressed timing → insufficient consolidation cycles
3. The simulator ran 100 seeds; KWOK ran 1 iteration per variant

---

## Root Cause Summary

| Root Cause | Evidence | Impact |
|------------|----------|--------|
| KWOK nodes not reporting Ready/allocatable | All pods Pending throughout all runs | Pods never scheduled → no consolidation possible |
| Scale mismatch (50 vs 500 replicas) | Fast-mode script uses 50 replicas | Even with working scheduling, results not comparable |
| Timing too short (~2min vs 35min) | Fast-mode window insufficient for consolidateAfter cycles | Consolidation may not trigger even with scheduled pods |
| CRD field recognition uncertain | All CostJustified variants show 1 node (no provisioning) | PR #2893 fields may not be in the Karpenter build |
| Single iteration per variant | No summary for cost-justified-1.50; others ran once | No statistical confidence; run-to-run variance unknown |

---

## Recommendations

### R1: Fix KWOK Node Readiness (Critical)

Before any further verification, KWOK nodes must report `Ready` condition and
accurate `allocatable` resources. Verify with:

```bash
kubectl get nodes -o wide  # Check STATUS column
kubectl describe node <kwok-node> | grep -A5 Conditions
kubectl describe node <kwok-node> | grep -A5 Allocatable
```

If KWOK nodes show `NotReady` or zero allocatable resources, the KWOK controller
configuration needs adjustment. Check the KWOKNodeClass resource and KWOK
controller flags.

### R2: Verify CRD Schema (Critical)

Confirm that the Karpenter build from PR #2893 recognizes `consolidateWhen` and
`decisionRatioThreshold`:

```bash
kubectl get crd nodepools.karpenter.sh -o json | jq '.spec.versions[].schema.openAPIV3Schema.properties.spec.properties.disruption.properties'
```

If these fields are absent, the CostJustified variants were running with an
invalid/ignored NodePool spec.

### R3: Run Full-Mode Verification

Use `scripts/run-kwok-consolidate-verify.sh` (35-minute sequence, 500 replicas)
instead of the fast-mode script. The fast mode was designed for pipeline
validation, not for producing comparable results.

### R4: Match Simulator Scale

Set replicas to 500 per deployment to match the simulator's benchmark-control
scenario. The scale-sequence.sh (not scale-sequence-fast.sh) already uses the
correct timing.

### R5: Add Pod Scheduling Verification Gate

Add a check to the orchestrator script that verifies pods transition from
`Pending` to `Running` before proceeding with the scale sequence:

```bash
# After deploying workloads, wait for at least 1 pod to be Running
kubectl wait --for=condition=Ready pod -l app=workload-a --timeout=60s
```

If this gate fails, the variant should be marked as `infra-failure` rather than
producing misleading 0-eviction results.

### R6: Simulator Calibration (Deferred)

No simulator calibration adjustments are warranted from this data. Once R1-R5
are addressed and a valid full-mode run completes, re-run the comparison using
`scripts/compare-kwok-vs-simulator.py` and evaluate against the tolerances
defined in the verification plan (§6.1):
- Node count: ±15%
- Evictions: ±3 absolute
- Total cost: ±10%
- Structural ordering: Spearman ρ ≥ 0.85

---

## What the Data Does Show

Despite the infrastructure failures, the KWOK data reveals one useful signal:

**Provisioning behavior differs by policy type.** WhenEmpty and WhenUnderutilized
both triggered Karpenter to provision 14 nodes for 100 pods, while all
CostJustified variants stayed at 1 node. This suggests the CostJustified
NodePool specs were either rejected (CRD mismatch) or caused Karpenter to
enter a different provisioning path. This is worth investigating independently
of the consolidation comparison.

The simulator correctly predicts that CostJustified variants should have fewer
nodes than WhenEmpty (3 vs 6), and the KWOK data shows the same directional
relationship (1 vs 14), though the magnitudes are not comparable due to the
infrastructure issues.
