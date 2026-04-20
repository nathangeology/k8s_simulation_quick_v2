# KWOK Comparison: Baseline vs Pod-Deletion-Cost Controller

**Date:** 2026-04-20
**Cluster:** KIND + KWOK (kubesim)
**NodePool:** default (WhenEmpty, no instance type filter, 10% disruption budget)
**Workloads:** workload-a (950m CPU, 3.5Gi mem) + workload-b (950m CPU, 6.5Gi mem)

## Results

| Metric | Run 1: PodDeletionCost=true | Run 2: PodDeletionCost=false |
|--------|----------------------------|------------------------------|
| Nodes at peak (1000 pods) | 5 | 7 |
| Nodes after 700 pods (350+350) | 3 | 7 |
| Empty nodes after 700 pods | 0 | 0 |
| Nodes after 20 pods (10+10) | 1 | 3 |
| Final nodes after 5 min consolidation | 1 | 3 |
| Total disruption events | 4 | 4 |

## Node Types at Peak

**Run 1 (PodDeletionCost=true):**
- 3× m-256x-amd64-linux
- 1× s-256x-amd64-linux
- 1× m-32x-amd64-linux

**Run 2 (PodDeletionCost=false):**
- 1× m-256x-amd64-linux
- 2× s-256x-amd64-linux
- 1× m-128x-amd64-linux
- 1× s-64x-amd64-linux
- 1× s-48x-amd64-linux
- 1× s-32x-amd64-linux

## Analysis

### Provisioning Efficiency

With PodDeletionCost enabled, Karpenter provisioned **5 nodes** (mostly large m-256x instances)
to handle 1000 pods. Without it, Karpenter provisioned **7 nodes** with a more fragmented mix
of instance types. The deletion cost controller's pod annotations appear to influence Karpenter's
bin-packing decisions, resulting in tighter packing on fewer, larger nodes.

### Consolidation Behavior

The most significant difference is in consolidation after aggressive scale-down:

- **Run 1 (enabled):** After scaling from 1000→700 pods, 2 nodes became empty and were removed
  (5→3 nodes). After scaling to 20 pods, all remaining excess nodes emptied and were removed,
  reaching **1 final node**.

- **Run 2 (disabled):** After scaling from 1000→700 pods, pods remained spread across all 7 nodes
  with none becoming empty. After scaling to 20 pods, only 4 of 7 nodes became empty and were
  removed, leaving **3 final nodes** with pods still scattered (1, 9, and 10 pods respectively).

### Why the Difference

With PodDeletionCostManagement=true, the controller annotates pods with deletion costs that
guide the ReplicaSet controller to preferentially delete pods from nodes that are already
draining or underutilized. This creates a "snowball" effect: as pods are removed from a node,
remaining pods on that node get lower deletion costs, making them more likely to be deleted
next. This concentrates surviving pods onto fewer nodes, leaving more nodes fully empty for
WhenEmpty consolidation.

Without the controller, pod deletion during scale-down is essentially random across nodes.
Pods are removed uniformly, so no node becomes fully empty until the total pod count is very
low. This leaves "stragglers" — nodes with just a few pods that WhenEmpty cannot remove.

### Cost Implications

Run 1 achieved full consolidation to 1 node. Run 2 left 3 nodes running with only 20 total
pods. In a real cluster, those 2 extra nodes represent wasted compute cost. The effect would
compound with more heterogeneous workloads and larger clusters.

## Disruption Events

Both runs had 4 disruption events (all Empty/delete type). The difference is *when* they
occurred relative to scale-down:

- **Run 1:** 2 events after 1000→700, 2 events after 700→20
- **Run 2:** 0 events after 1000→700 (no empty nodes), 4 events after 700→20

## Conclusion

PodDeletionCostManagement significantly improves WhenEmpty consolidation efficiency by
concentrating pod deletions onto specific nodes during scale-down. Without it, the WhenEmpty
policy struggles to consolidate because pods remain scattered across nodes, preventing any
single node from becoming fully empty until extreme scale-down ratios.
