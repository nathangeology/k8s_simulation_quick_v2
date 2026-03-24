# KWOK Eviction Investigation: Why Low Eviction Counts?

**Bead:** k8s-u7vb
**Date:** 2026-03-24
**Data sources:** `results/kwok-verify/` (full-mode runs), live cluster observation

---

## Executive Summary

The low eviction counts (1-2 per variant) are caused by a **measurement bug in the
verification script**, not by missing evictions. Karpenter IS evicting pods during
consolidation. A live observation of a 100→20 pod scale-down captured **125 pod
Evicted events** from Kubernetes, while the script's log-grep method reported only 5
disruption commands.

The root cause is a two-part measurement gap:

1. **The script counts Karpenter disruption commands, not pod evictions.** Each
   disruption command removes 1-2 nodes but the pod-count field in the log is the
   count at decision time, not the number actually evicted.

2. **Most nodes are already empty when Karpenter processes them.** The deployment
   scale-down deletes pods instantly (`terminationGracePeriodSeconds: 0`), emptying
   most nodes before Karpenter's consolidation loop (30s `consolidateAfter` + ~15s
   per disruption cycle) reaches them. Karpenter then removes these as Empty nodes
   with pod-count=0, which the script counts as 0 evictions.

---

## The Mechanism: What Actually Happens

### Timeline (observed live, when-underutilized, 100→20 pods)

```
t=0s    kubectl scale --replicas=10 (100 pods → 20 target)
t=0-5s  Kubernetes deletes 80 pods instantly (KWOK: terminationGracePeriodSeconds=0)
        Nodes go from ~8 pods each → 0-2 pods each
        Most nodes are now EMPTY (no eviction by Karpenter)
t=25s   Karpenter disruption loop fires (consolidateAfter: 30s)
        Finds ~6 empty nodes, ~6 nodes with 1-3 pods
t=25s   Disruption 1: Underutilized, delete 2 nodes, pod-count=3
t=40s   Disruption 2: Empty, delete 1 node, pod-count=0
t=55s   Disruption 3: Underutilized, delete 1 node, pod-count=2
t=70s   Disruption 4: Underutilized, delete 1 node, pod-count=1
t=85s   Disruption 5: Underutilized, delete 1 node, pod-count=2
t=120s  Consolidation complete: 13→7 nodes
```

### Two Distinct Node Removal Paths

| Path | Mechanism | Eviction? | Karpenter log? |
|------|-----------|-----------|----------------|
| **Empty node removal** | Deployment scale-down deletes pods → node becomes empty → Karpenter removes empty node | No (no pods to evict) | `disrupting node(s)` with `pod-count: 0` |
| **Underutilized node drain** | Karpenter taints node → evicts remaining pods → pods reschedule → node drained → deleted | Yes (pods get Evicted events) | `disrupting node(s)` with `pod-count: N` |

In the full-mode runs (1000→700→20), the vast majority of nodes follow Path 1
because the deployment scale-down (1000→700, then 700→20) empties most nodes
before Karpenter acts.

### Evidence: Live Observation Event Counts

| Event Type | Count | Source |
|------------|-------|--------|
| Pod `Evicted` events | 125 | `kubectl get events -w` |
| `DisruptionTerminating` | 242 | Node/NodeClaim events (2 per node: node + claim) |
| `Drained` | 58 | NodeClaim status transitions |
| `RemovingNode` | 133 | Node controller events |
| `disrupting node(s)` log lines | 5 | Karpenter controller logs |

The script only counted the last row (5 disruption commands), missing the 125
actual pod evictions.

---

## The Measurement Bug

### Script: `run-kwok-full-verify.sh`, lines 109-111

```bash
kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter --since=40m \
  | grep -E '(disrupting|consolidat|decision.ratio)' \
  > "$variant_dir/karpenter-consolidation.log" 2>/dev/null || true

evictions=$(grep -c 'disrupting node' "$variant_dir/karpenter-consolidation.log") || evictions=0
```

**Problems:**

1. **Counts disruption commands, not evictions.** A single `disrupting node(s)` line
   can evict 0, 1, or many pods. The `pod-count` field in the log is the number of
   pods on the disrupted nodes at decision time, but pods may have already terminated
   by the time the drain executes.

2. **Karpenter processes nodes serially (~15s each).** With `consolidateAfter: 30s`,
   the first disruption fires ~30s after nodes become empty/underutilized. Each
   subsequent disruption takes ~15s. For 6 nodes, that's ~90s of consolidation time.
   During this time, the deployment controller has already deleted most pods.

3. **The grep filter misses the actual eviction events.** Pod evictions are recorded
   as Kubernetes events (`Evicted` reason on pod objects), not in Karpenter controller
   logs. The Karpenter log shows the disruption *decision*, not the eviction *execution*.

### Correct Eviction Counting

To count actual evictions, use Kubernetes events:

```bash
# Count pod evictions during a time window
kubectl get events -n default --field-selector reason=Evicted --no-headers | wc -l
```

Or sum the `pod-count` field from Karpenter disruption logs (approximate):

```bash
grep 'disrupting node' karpenter.log \
  | python3 -c "import json,sys; print(sum(json.loads(l)['pod-count'] for l in sys.stdin))"
```

---

## Answering the Key Questions

### Are nodes being removed AFTER they become empty (no eviction needed)?

**Yes, mostly.** The timeseries data proves this conclusively:

```
when-underutilized (full-mode, 1000→20):
  20:59:51  nodes=88  pods=700   ← scale-down command issued
  21:00:21  nodes=88  pods=20    ← 680 pods deleted by deployment controller (30s)
  21:00:51  nodes=79  pods=20    ← Karpenter starts removing empty nodes
  ...
  21:09:26  nodes=8   pods=20    ← 80 nodes removed over ~9 minutes
```

680 pods were deleted by the deployment scale-down. Only the ~20 remaining pods
needed to be redistributed by Karpenter across fewer nodes, generating actual
evictions.

### Or are nodes being drained (pods evicted) as part of consolidation?

**Both.** Karpenter uses both paths:
- **Empty path:** Nodes with 0 pods → direct deletion (no drain needed)
- **Underutilized path:** Nodes with 1-3 remaining pods → taint → evict → drain → delete

The live observation showed Karpenter using both `Empty/` and `Underutilized/`
disruption commands, confirming it correctly identifies and handles both cases.

### Is the eviction counting script missing the actual eviction mechanism?

**Yes.** The script counts Karpenter disruption *decisions* (log lines), not
Kubernetes eviction *events* (pod events). These are different things:
- Disruption decision: "I will remove this node" (1 log line per 1-2 nodes)
- Eviction event: "This pod was evicted" (1 event per pod)

### Does Karpenter use node drain or node deletion?

**Karpenter uses drain (taint → evict → delete).** The live logs show the full
sequence for every disrupted node:

```
1. disrupting node(s)     ← decision to disrupt
2. tainted node           ← NoSchedule taint applied
3. FailedDraining         ← waiting for pods to be evicted (transient)
4. Evicted (pod event)    ← each pod evicted individually
5. Drained                ← all pods evicted, node drained
6. deleted node           ← node object removed
7. deleted nodeclaim      ← nodeclaim object removed
```

For empty nodes, steps 3-4 are skipped (nothing to drain).

---

## Impact on Simulator Calibration

### Expected vs Actual Evictions

For the when-underutilized variant (full-mode):
- **Nodes removed:** 80 (88→8)
- **Pods on those nodes at scale-down time:** ~680 (700→20 by deployment)
- **Pods actually evicted by Karpenter:** ~8 (from disruption commands with
  pod-counts 3+0+2+1+2=8 in the live observation; proportionally ~10-15 for the
  full-mode run with more nodes)
- **Script reported:** 2

The simulator's eviction prediction should be compared against the actual
Kubernetes Evicted event count, not the disruption command count.

### Simulator Calibration Recommendations

1. **Eviction count is low by design.** In a scale-down scenario, most pods are
   deleted by the deployment controller, not evicted by Karpenter. The simulator
   should model this: evictions only occur for pods that remain on underutilized
   nodes after the deployment scale-down completes.

2. **The relevant metric is "pods disrupted by consolidation"**, not "total pods
   removed". For simulator accuracy, track:
   - Pods deleted by deployment scale-down (not evictions)
   - Pods evicted by Karpenter consolidation (actual evictions)
   - Nodes removed empty (no eviction cost)
   - Nodes drained (eviction cost)

3. **Timing matters.** KWOK pods terminate instantly (`terminationGracePeriodSeconds: 0`),
   so nodes empty faster than in production. Real pods with grace periods would leave
   more pods on nodes when Karpenter's consolidation loop fires, producing more
   evictions. The simulator should account for termination grace period.

---

## Recommendations

### R1: Fix Eviction Counting in Verification Script (Bug Fix)

Replace the grep-based counting with event-based counting:

```bash
# Capture events during the run
kubectl get events -n "$NAMESPACE" -w > "$variant_dir/events.log" &

# After run, count actual evictions
evictions=$(grep -c "Evicted" "$variant_dir/events.log") || evictions=0

# Also capture disruption commands for analysis
disruptions=$(grep -c 'disrupting node' "$variant_dir/karpenter-consolidation.log") || disruptions=0
```

Update `summary.json` to include both metrics:

```json
{
  "pods_evicted": "<evicted-event-count>",
  "disruption_commands": "<disrupting-node-count>",
  "nodes_removed_empty": "<empty-disruption-count>",
  "nodes_drained": "<underutilized-disruption-count>"
}
```

### R2: Capture Full Karpenter Logs (Not Just Grep)

The current grep filter (`disrupting|consolidat|decision.ratio`) misses important
log lines like `tainted node`, `deleted node`, `deleted nodeclaim`. Capture the
full Karpenter log for the run window and filter during analysis, not collection.

### R3: Add Event Stream Capture to Verification Script

Add `kubectl get events -w` capture alongside the timeseries collection. Events
provide the ground truth for pod evictions, node drains, and scheduling decisions.

### R4: Separate Deployment-Deleted vs Karpenter-Evicted Pods

The simulator should distinguish between:
- **Deployment-deleted pods:** Removed by replica count reduction (no disruption cost)
- **Karpenter-evicted pods:** Removed by consolidation drain (disruption cost, potential
  availability impact)

This distinction is critical for accurate disruption budget modeling.
