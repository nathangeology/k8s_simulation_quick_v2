# KubeSim Accuracy Audit vs Upstream

**Date:** 2026-03-11
**Scope:** kubesim-scheduler, kubesim-karpenter vs kube-scheduler (v1.31+) and Karpenter v1.x
**Bead:** k8s-5y7

---

## 1. Scheduler (kubesim-scheduler)

### 1.1 Filter Phase

#### Implemented Filters
| Filter | Status | Notes |
|--------|--------|-------|
| NodeResourcesFit | ✅ Implemented | Checks allocatable − allocated vs pod requests |
| TaintToleration | ✅ Implemented | Iterates node taints, checks pod tolerations |
| NodeAffinity | ✅ Implemented | Required terms only in filter; preferred in scorer |
| InterPodAffinity | ✅ Implemented | Required affinity + anti-affinity in filter |
| PodTopologySpread | ✅ Implemented | DoNotSchedule constraint enforcement |

#### Missing Filters (upstream kube-scheduler)
| Filter | Impact | Upstream Reference |
|--------|--------|--------------------|
| **NodeUnschedulable** | LOW — kubesim checks `node.cordoned` inline in `schedule_one()`, but upstream has a dedicated filter plugin that checks `node.spec.unschedulable`. Functionally equivalent for now. | `pkg/scheduler/framework/plugins/nodeunschedulable/` |
| **NodePorts** | MEDIUM — upstream rejects nodes where requested hostPorts conflict with already-bound ports. kubesim has no hostPort modeling at all. Affects workloads using hostNetwork/hostPort. | `pkg/scheduler/framework/plugins/nodeports/` |
| **VolumeBinding** | HIGH — upstream checks PV/PVC binding, storage class topology, and volume limits per node. kubesim has no storage modeling. Any workload with PVCs will have inaccurate placement. | `pkg/scheduler/framework/plugins/volumebinding/` |
| **VolumeRestrictions** | MEDIUM — prevents conflicting volume mounts (e.g., two pods mounting same RWO PVC). No storage model in kubesim. | `pkg/scheduler/framework/plugins/volumerestrictions/` |
| **NodeVolumeLimits** | MEDIUM — enforces per-node volume attachment limits (e.g., 25 EBS volumes on AWS). | `pkg/scheduler/framework/plugins/nodevolumelimits/` |
| **EBSLimits / GCEPDLimits / AzureDiskLimits** | LOW for AWS-only sim — cloud-specific volume limits. | `pkg/scheduler/framework/plugins/nodevolumelimits/` |
| **PodTopologySpread (minDomains)** | LOW — upstream supports `minDomains` field (v1.30 GA). kubesim's filter doesn't check it. Affects clusters with auto-scaling where domains appear/disappear. | `pkg/scheduler/framework/plugins/podtopologyspread/filtering.go` |

#### Filter Accuracy Issues

1. **NodeResourcesFit — extended resources not modeled.** Upstream checks all extended resources (e.g., `nvidia.com/gpu`, `hugepages-2Mi`, custom device plugins). kubesim only checks cpu, memory, gpu (as a flat u32), and ephemeral storage. GPU is modeled as a count but not as a named extended resource, so heterogeneous GPU types (A100 vs T4) can't be distinguished at the resource level — only via labels/taints.
   - Upstream: `pkg/scheduler/framework/plugins/noderesources/fit.go` — iterates `pod.Spec.Containers[*].Resources.Requests` for ALL resource names.

2. **TaintToleration — PreferNoSchedule not filtered.** This is correct — upstream also only filters NoSchedule and NoExecute in the filter phase. PreferNoSchedule is handled in scoring. ✅

3. **NodeAffinity — matchExpressions not supported.** kubesim only supports `matchLabels` (exact key=value). Upstream supports `matchExpressions` with operators: `In`, `NotIn`, `Exists`, `DoesNotExist`, `Gt`, `Lt`. This is a significant gap for workloads using set-based selectors.
   - Upstream: `staging/src/k8s.io/apimachinery/pkg/labels/selector.go`

4. **InterPodAffinity — namespaceSelector not modeled.** Upstream pod affinity terms can scope to specific namespaces or use `namespaceSelector`. kubesim has no namespace concept, so all pods are in a single flat namespace. This means anti-affinity is overly broad (matches across what would be separate namespaces upstream).
   - Upstream: `pkg/scheduler/framework/plugins/interpodaffinity/filtering.go`

5. **PodTopologySpread — nodeInclusionPolicy missing.** Upstream (v1.26+) supports `nodeInclusionPolicy` (`Honor`/`Ignore`) controlling whether node taints/labels affect domain counting. kubesim counts all nodes regardless.
   - Upstream: `pkg/scheduler/framework/plugins/podtopologyspread/filtering.go`

### 1.2 Score Phase

#### Implemented Scorers
| Scorer | Status | Notes |
|--------|--------|-------|
| MostAllocated | ✅ | Averages CPU + memory utilization % |
| LeastAllocated | ✅ | 100 − MostAllocated score |
| NodeAffinityScore | ✅ | Sums weights of matching preferred terms |
| InterPodAffinityScore | ✅ | Weighted count of matching pods in topology domain |
| PodTopologySpreadScore | ✅ | Penalizes skew for ScheduleAnyway constraints |

#### Scoring Accuracy Issues

1. **MostAllocated/LeastAllocated — only CPU + memory averaged.** Upstream `NodeResourcesFit` scorer (which replaced the old `MostAllocated`/`LeastAllocated` plugins in v1.27+) considers ALL requested resource types with configurable per-resource weights. The default weights CPU and memory equally, but operators can add GPU, ephemeral storage, etc.
   - Upstream: `pkg/scheduler/framework/plugins/noderesources/requested_to_capacity_ratio.go`
   - Impact: MEDIUM — GPU-heavy workloads won't see GPU utilization factored into scoring.

2. **BalancedAllocation scorer missing.** Upstream has `BalancedAllocation` which penalizes nodes where CPU and memory utilization are imbalanced (e.g., 90% CPU but 10% memory). This is a default scorer in the standard profile.
   - Upstream: `pkg/scheduler/framework/plugins/noderesources/balanced_allocation.go`
   - Impact: MEDIUM — without this, bin-packing can create nodes with lopsided resource usage, leading to stranded resources.

3. **TaintToleration scorer missing.** Upstream scores PreferNoSchedule taints — nodes with fewer unmatched PreferNoSchedule taints get higher scores.
   - Upstream: `pkg/scheduler/framework/plugins/tainttoleration/taint_toleration.go`
   - Impact: LOW — only matters when PreferNoSchedule taints are used.

4. **ImageLocality scorer missing.** Upstream prefers nodes that already have container images cached. Not relevant for a cost/scheduling simulator unless image pull latency is modeled.
   - Impact: LOW for kubesim's use case.

5. **Score normalization missing.** Upstream normalizes all scores to [0, 100] range via `NormalizeScore` extension point before applying weights. kubesim's scorers return raw values — `InterPodAffinityScore` returns unbounded counts, `NodeAffinityScore` returns sum of weights (could exceed 100). This means scorer weights don't compose correctly.
   - Upstream: `pkg/scheduler/framework/interface.go` — `NormalizeScore` is called after `Score`.
   - Impact: HIGH — when multiple scorers are active, the one with the largest raw range dominates regardless of configured weights.

### 1.3 Preemption

#### Implemented
- Priority-based victim selection (lowest priority first) ✅
- Minimal victim set (greedy, stop when enough resources freed) ✅
- PDB violation counting ✅
- Candidate ranking: (PDB violations, num victims, total victim priority) ✅
- Skips NodeResourcesFit filter during preemption evaluation ✅

#### Gaps

1. **No nominated node tracking.** Upstream sets `pod.Status.NominatedNodeName` on the preemptor and reserves capacity on the nominated node for future scheduling cycles. kubesim binds immediately after eviction in the same `schedule_pending` call, which is optimistic — in reality, eviction is async and the preemptor may wait multiple cycles.
   - Upstream: `pkg/scheduler/framework/preemption/preemption.go`
   - Impact: MEDIUM — affects scheduling latency accuracy for preemption scenarios.

2. **No graceful termination period for victims.** Upstream respects `terminationGracePeriodSeconds` — victims aren't instantly removed. kubesim's `evict_pod` immediately frees resources and returns the pod to Pending. This makes preemption appear faster than reality.
   - Impact: MEDIUM for wall-clock time mode studies.

3. **InterPodAffinity not re-evaluated post-eviction.** After removing victims, the topology domain's pod set changes. Upstream re-runs affinity checks. kubesim doesn't — it only skips `NodeResourcesFit` during preemption.
   - Impact: LOW — edge case where evicting a victim breaks affinity for the preemptor.

### 1.4 Scheduling Loop

#### Implemented
- Priority-ordered scheduling (descending) ✅
- Single-pod-at-a-time scheduling ✅

#### Gaps

1. **No scheduling queue with backoff.** Upstream uses `activeQ`, `backoffQ`, and `unschedulableQ` with exponential backoff. Pods that fail scheduling are retried with increasing delays. kubesim retries all pending pods every cycle with no backoff.
   - Upstream: `pkg/scheduler/internal/queue/scheduling_queue.go`
   - Impact: LOW for correctness, MEDIUM for latency accuracy in wall-clock mode.

2. **No scheduling cycle vs binding cycle separation.** Upstream has a two-phase commit: the scheduling cycle (filter+score) runs with a read snapshot, then the binding cycle (reserve+bind) runs separately and can fail. kubesim does both atomically.
   - Impact: LOW — mainly affects concurrent scheduling accuracy.

---

## 2. Karpenter (kubesim-karpenter)

### 2.1 Provisioning

#### Implemented
- Pending pod batching by compatible constraints ✅
- Instance type selection (cheapest that fits) ✅
- NodePool limits enforcement (max nodes, CPU, memory) ✅
- Multi-batch provisioning with running usage tracking ✅

#### Gaps

1. **Batching is over-simplified.** kubesim groups pods by (required labels, toleration keys, GPU count). Upstream Karpenter uses full scheduling simulation — it creates a "virtual node" per candidate instance type and runs the scheduler's filter chain against each pending pod to determine compatibility. This means kubesim may:
   - Over-batch: group pods that actually have incompatible constraints (e.g., different topology spread requirements).
   - Under-batch: fail to group pods that are compatible but have different label sets that happen to be satisfied by the same instance type.
   - Upstream: `pkg/controllers/provisioning/scheduling/scheduler.go` — `NewScheduler().Solve()`
   - Impact: HIGH — directly affects node count and cost accuracy.

2. **No multi-NodePool support.** kubesim's `provision()` takes a single `NodePool`. Upstream Karpenter evaluates ALL NodePools simultaneously and picks the cheapest option across pools. A cluster with GPU and general-purpose pools would need manual orchestration in kubesim.
   - Upstream: `pkg/controllers/provisioning/provisioner.go` — iterates all NodePools
   - Impact: MEDIUM — affects multi-pool scenarios.

3. **No NodeClaim lifecycle.** Upstream Karpenter creates `NodeClaim` objects that go through `Pending → Launched → Registered → Initialized → Ready` states. kubesim emits `NodeLaunching` and expects the engine to handle `NodeReady`, but there's no intermediate state machine. This means:
   - No modeling of launch failures or retries.
   - No `--node-startup-timeout` behavior.
   - Upstream: `pkg/controllers/nodeclaim/lifecycle/controller.go`
   - Impact: LOW for cost studies, MEDIUM for latency studies.

4. **Instance type selection is purely cost-based.** Upstream Karpenter scores instance types considering: price, available capacity zones, architecture compatibility, and a "flexibility" score that prefers instance types with more availability. kubesim picks the cheapest that fits.
   - Upstream: `pkg/controllers/provisioning/scheduling/instance_type.go`
   - Impact: MEDIUM — in practice, cheapest-first is a reasonable approximation but misses zone-awareness.

5. **No spot instance selection logic.** Upstream Karpenter's `capacity-type` requirement allows mixing on-demand and spot. The provisioner uses price-capacity-optimized allocation strategy for spot. kubesim's provisioner only considers `on_demand_price_per_hour` and doesn't model spot selection at all (spot is only modeled for interruption in `spot.rs`).
   - Upstream: `pkg/providers/instancetype/instancetype.go`
   - Impact: HIGH for spot-heavy workload studies.

6. **No pod anti-affinity awareness in batching.** If pod A has anti-affinity to pod B, they shouldn't be batched onto the same node. kubesim's batching doesn't check this.
   - Impact: MEDIUM — can produce infeasible placements that the scheduler would reject.

### 2.2 Consolidation

#### Implemented
- WhenEmpty: terminate nodes with zero pods ✅
- WhenUnderutilized: greedy first-fit reschedulability check ✅
- Disruption budget (percentage-based) ✅
- Cost-ordered candidate evaluation (cheapest first) ✅

#### Gaps

1. **Candidate sorting is simplified.** Upstream Karpenter v1.x sorts consolidation candidates by a multi-factor score:
   - Disruption cost (considers pod priorities, PDBs, `do-not-disrupt` annotation)
   - Node age (older nodes preferred for replacement)
   - Number of pods (fewer pods = less disruption)
   - kubesim sorts only by `cost_per_hour`.
   - Upstream: `pkg/controllers/disruption/consolidation.go` — `SortCandidates()`
   - Impact: HIGH — directly affects which nodes get consolidated and disruption patterns.

2. **No `do-not-disrupt` / `do-not-consolidate` annotation.** Upstream respects `karpenter.sh/do-not-disrupt` on nodes and pods. kubesim has no mechanism to exempt nodes/pods from consolidation.
   - Upstream: `pkg/controllers/disruption/helpers.go`
   - Impact: MEDIUM — affects workloads that need stability guarantees.

3. **No replacement node evaluation.** Upstream's `WhenUnderutilized` doesn't just check if pods fit on existing nodes — it also evaluates whether launching a smaller replacement node would be cheaper. This is the "consolidation by replacement" path. kubesim only does "consolidation by deletion" (move pods to existing nodes).
   - Upstream: `pkg/controllers/disruption/consolidation.go` — `computeConsolidation()` has both `delete` and `replace` paths.
   - Impact: HIGH — missing the replace path means kubesim underestimates consolidation savings. A node running 3 pods that don't fit elsewhere but could fit on a smaller instance type won't be consolidated.

4. **Reschedulability check is resource-only.** `pods_can_reschedule()` only checks if resources fit on other nodes. It doesn't verify scheduling constraints (affinity, taints, topology spread). A pod with a node affinity that only matches the current node would be incorrectly deemed reschedulable.
   - Impact: HIGH — can produce consolidation decisions that would fail in practice.

5. **No consolidation cooldown / TTL.** Upstream has `consolidateAfter` (time a node must be consolidatable before action is taken) and `consolidationPolicy` per NodePool. kubesim acts immediately on every loop.
   - Upstream: `pkg/apis/v1/nodepool_types.go` — `ConsolidateAfter`
   - Impact: MEDIUM — affects disruption frequency accuracy.

6. **Disruption budget is percentage-only.** Upstream supports both absolute count and percentage, plus per-NodePool budgets with schedule-based overrides (e.g., "allow 100% disruption during maintenance windows"). kubesim only has a flat percentage.
   - Upstream: `pkg/apis/v1/nodepool_types.go` — `Budget` struct with `Nodes`, `Schedule`, `Duration`
   - Impact: LOW-MEDIUM — depends on scenario complexity.

7. **No multi-node consolidation.** Upstream can evaluate consolidating multiple nodes simultaneously (e.g., moving pods from nodes A and B onto node C). kubesim evaluates one node at a time.
   - Upstream: `pkg/controllers/disruption/multinodeconsolidation/` 
   - Impact: MEDIUM — misses optimization opportunities.

### 2.3 Drift Detection

#### Implemented
- Instance type drift (node type no longer in NodePool spec) ✅
- PDB-respecting drain ✅
- Drain timeout with force eviction ✅
- Provisioning loop trigger after termination ✅

#### Gaps

1. **Only instance type drift detected.** Upstream detects drift on multiple axes:
   - Instance type changes in NodePool
   - Label/annotation changes on NodePool
   - AMI/image changes (NodePool `amiFamily` or `amiSelector`)
   - Subnet/security group changes
   - kubelet configuration changes
   - Upstream: `pkg/controllers/disruption/drift.go` and `pkg/cloudprovider/` drift checks
   - Impact: MEDIUM — instance type drift is the most common, but AMI drift is important for security patching scenarios.

2. **No drift hash comparison.** Upstream computes a hash of the NodePool spec and stores it on the NodeClaim. Drift is detected by hash mismatch. kubesim does a direct instance type string comparison.
   - Impact: LOW — functionally similar for the instance type case.

3. **Cordoning not actually applied.** The drift handler emits `NodeCordoned` events but doesn't set `node.cordoned = true` before attempting drain. The consolidation handler does set it. This means during drift drain, the scheduler could still place new pods on the drifting node.
   - Impact: MEDIUM — race condition in drift scenarios.

### 2.4 Spot Interruption

#### Implemented
- Stochastic per-node interruption probability ✅
- 2-minute ITN warning period ✅
- Pod eviction back to pending queue ✅
- Node termination after grace period ✅

#### Gaps

1. **Interruption probability is static.** Real spot interruption rates vary by instance type, AZ, and time. Upstream Karpenter doesn't model probability — it reacts to actual ITN events from the EC2 metadata service. kubesim's stochastic model is a reasonable simulation choice but the per-step probability doesn't account for:
   - Correlated interruptions (spot capacity pool exhaustion affects multiple nodes)
   - Time-varying rates (higher during peak hours)
   - Impact: MEDIUM — for statistical studies, independent per-node probability may underestimate tail disruption events.

2. **No Spot rebalance recommendation.** AWS sends rebalance recommendations before interruptions. Karpenter can proactively replace nodes on rebalance signals. kubesim only models the final interruption.
   - Upstream: Karpenter handles `EC2 Instance Rebalance Recommendation` events
   - Impact: LOW — rebalance is an optimization, not a correctness issue.

3. **Spot handler piggybacks on KarpenterProvisioningLoop.** This means spot checks only happen when provisioning loops fire. If provisioning stops (no pending pods), spot interruptions stop being checked. Should have its own periodic event.
   - Impact: MEDIUM — in steady state with no pending pods, spot interruptions won't fire.

---

## 3. NodePool Selection and Constraints

### Implemented
- Single NodePool with instance type allowlist ✅
- NodePool limits (max nodes, CPU, memory) ✅
- NodePool labels and taints ✅
- Disruption budget percentage ✅

### Gaps

1. **No multi-NodePool orchestration.** Real clusters have multiple NodePools (e.g., general, GPU, spot, arm64). Karpenter evaluates all pools for each provisioning decision. kubesim's `ProvisioningHandler` takes a single pool.
   - Impact: HIGH for heterogeneous cluster studies.

2. **NodePool weight/priority not modeled.** Upstream NodePools have a `weight` field that influences which pool is preferred when multiple pools can satisfy a request.
   - Upstream: `pkg/apis/v1/nodepool_types.go` — `Weight`
   - Impact: LOW-MEDIUM.

3. **No NodePool-level consolidation policy.** Upstream allows per-NodePool `consolidationPolicy` and `consolidateAfter`. kubesim uses a single global policy.
   - Impact: MEDIUM for multi-pool scenarios.

4. **NodePool labels/taints not applied to launched nodes.** The `NodePool` struct has `labels` and `taints` fields, but `ProvisioningHandler.handle()` emits `NodeLaunching(NodeSpec { instance_type })` — the NodeSpec only carries instance type, not the pool's labels/taints. Launched nodes won't have the pool's labels, breaking scheduling constraints that depend on them.
   - Impact: HIGH — nodes launched from a tainted pool won't actually be tainted.

---

## 4. Summary: Priority-Ranked Gaps

### Critical (affects core simulation accuracy)

| # | Gap | Component | Fix Complexity |
|---|-----|-----------|---------------|
| 1 | Score normalization missing — scorers don't compose correctly | scheduler | LOW — add normalize pass |
| 2 | Consolidation reschedulability ignores scheduling constraints | karpenter/consolidation | MEDIUM — run scheduler filters |
| 3 | Consolidation missing "replace" path (only does "delete") | karpenter/consolidation | HIGH — needs instance selection |
| 4 | Provisioning batching doesn't use scheduler simulation | karpenter/provisioner | HIGH — needs virtual node approach |
| 5 | NodePool labels/taints not propagated to launched nodes | karpenter/handler | LOW — extend NodeSpec |
| 6 | Spot check piggybacks on provisioning loop (stops in steady state) | karpenter/spot | LOW — add own periodic event |

### High (affects specific study types)

| # | Gap | Component | Fix Complexity |
|---|-----|-----------|---------------|
| 7 | No matchExpressions in node affinity (only matchLabels) | scheduler | MEDIUM |
| 8 | Consolidation candidate sorting is cost-only (not multi-factor) | karpenter/consolidation | MEDIUM |
| 9 | No multi-NodePool support | karpenter/provisioner | HIGH |
| 10 | No spot instance selection in provisioner | karpenter/provisioner | MEDIUM |
| 11 | No BalancedAllocation scorer | scheduler | LOW |
| 12 | No storage/volume modeling | scheduler | HIGH |

### Medium (affects edge cases or latency accuracy)

| # | Gap | Component |
|---|-----|-----------|
| 13 | No nominated node tracking in preemption | scheduler |
| 14 | No graceful termination period for evicted pods | scheduler |
| 15 | No scheduling queue backoff | scheduler |
| 16 | No consolidation cooldown / consolidateAfter | karpenter/consolidation |
| 17 | No do-not-disrupt annotation support | karpenter/consolidation |
| 18 | Drift only detects instance type changes | karpenter/drift |
| 19 | Drift handler doesn't cordon before drain attempt | karpenter/drift |
| 20 | No namespace concept (affects pod affinity scoping) | core |
| 21 | No multi-node consolidation | karpenter/consolidation |
| 22 | Correlated spot interruptions not modeled | karpenter/spot |

---

## 5. Recommendations for the Two Initial Studies

### Study 1: MostAllocated vs LeastAllocated

**Blocking gaps:** #1 (score normalization). With multiple scorers active, the MostAllocated/LeastAllocated score may be drowned out by InterPodAffinityScore or PodTopologySpreadScore returning unbounded values. Fix score normalization before running this study.

**Important gaps:** #11 (BalancedAllocation). This scorer is part of the default kube-scheduler profile and significantly affects node selection alongside MostAllocated/LeastAllocated. Without it, the comparison may not reflect real scheduler behavior.

**Acceptable simplifications:** Storage, namespaces, and preemption details are unlikely to affect the MostAllocated vs LeastAllocated comparison significantly.

### Study 2: Pod Deletion Cost for Node Draining

**Blocking gaps:** #2 (consolidation reschedulability ignores constraints), #3 (no replace path), #8 (candidate sorting). The deletion-cost study depends on accurate consolidation behavior. If consolidation incorrectly determines pods are reschedulable, or misses replace opportunities, the study results will be unreliable.

**Important gaps:** #5 (NodePool labels not propagated) — if the study uses NodePool taints, launched replacement nodes won't have them. #16 (no consolidateAfter) — affects timing of consolidation actions.

**Acceptable simplifications:** Spot interruption details, drift, and advanced scheduling features are secondary for this study.
