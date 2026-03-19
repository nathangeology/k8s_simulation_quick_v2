# ConsolidateWhen / Decision-Ratio Experiment Plan

**Bead:** k8s-b98n
**PR ref:** https://github.com/kubernetes-sigs/karpenter/pull/2893

---

## 1. Feature Summary

Karpenter PR #2893 introduces `NodePool.Spec.Disruption.ConsolidateWhen` with three policies:

| Policy | Behavior |
|--------|----------|
| `WhenEmpty` | Only consolidate nodes with zero non-daemonset pods |
| `WhenEmptyOrUnderutilized` | Existing default — consolidate empty + underutilized nodes |
| `WhenCostJustifiesDisruption` | Consolidate only when `decision_ratio > threshold` |

**Decision ratio** = `normalized_cost_savings / normalized_disruption_cost` per candidate node.

- `DecisionRatioThreshold` defaults to 1.0 (break-even).
- Values > 1.0 are more conservative (require larger savings to justify disruption).
- Values < 1.0 are more aggressive (accept smaller savings).

The PR also adds Prometheus metrics: `karpenter_consolidation_decision_ratio` (histogram) and decision counters.

---

## 2. Simulator Changes Required

### 2.1 Schema Changes (`kubesim-workload/src/scenario.rs`)

Extend `ConsolidationPolicy` enum and `ConsolidationConfig`:

```rust
pub enum ConsolidationPolicy {
    WhenEmpty,
    WhenUnderutilized,                // existing (maps to WhenEmptyOrUnderutilized)
    WhenEmptyOrUnderutilized,         // alias for clarity
    WhenCostJustifiesDisruption,      // new
}

pub struct ConsolidationConfig {
    pub policy: ConsolidationPolicy,
    pub decision_ratio_threshold: Option<f64>,  // new, default 1.0
}
```

### 2.2 Decision Ratio Calculator (`kubesim-karpenter/src/consolidation.rs`)

Add a `decision_ratio()` function that computes per-candidate:

```
normalized_cost_savings = (current_node_cost - replacement_cost) / max_node_cost_in_pool
normalized_disruption_cost = f(pod_count, pdb_coverage, priority, node_age)
decision_ratio = normalized_cost_savings / normalized_disruption_cost
```

The disruption cost normalization should reuse the existing `candidate_score()` factors:
- Pod count (more pods = higher disruption cost)
- PDB-covered pod count
- Max pod priority on the node
- Node age (newer nodes = higher disruption cost, since they were recently provisioned)

For `WhenCostJustifiesDisruption`, the consolidation evaluator should:
1. Compute `decision_ratio` for each candidate
2. Skip candidates where `decision_ratio < threshold`
3. Sort remaining candidates by decision ratio descending (best savings first)

### 2.3 Metrics Collection (`kubesim-metrics/src/collector.rs`)

Add to the per-tick snapshot:
- `consolidation_decisions_total` — count of candidates evaluated
- `consolidation_decisions_accepted` — count where ratio >= threshold
- `consolidation_decisions_rejected` — count where ratio < threshold
- `consolidation_decision_ratio_mean` — average ratio across candidates

### 2.4 Variant Support (`kubesim-workload/src/scenario.rs`)

Add `consolidate_when` as a variant-level override so the same scenario can compare policies:

```rust
pub struct Variant {
    // ... existing fields ...
    pub consolidate_when: Option<ConsolidateWhenVariant>,
}

pub struct ConsolidateWhenVariant {
    pub policy: ConsolidationPolicy,
    pub decision_ratio_threshold: Option<f64>,
}
```

### 2.5 Python Bindings (`kubesim-py/src/lib.rs`)

Map the new policy enum and threshold through to the Python `batch_run` interface so adversarial search scripts can vary these parameters.

---

## 3. Experiment Scenarios

### 3.1 Policy Comparison Matrix

**Goal:** Compare all 3 `ConsolidateWhen` policies across workload archetypes.

Each scenario uses 3 variants:
- `when-empty` — `ConsolidateWhen: WhenEmpty`
- `when-underutilized` — `ConsolidateWhen: WhenEmptyOrUnderutilized`
- `cost-justified` — `ConsolidateWhen: WhenCostJustifiesDisruption` (threshold=1.0)

| Scenario | Workload Profile | Expected Divergence |
|----------|-----------------|-------------------|
| `consolidate-when-web-steady` | 10 web apps, steady traffic, 8 replicas each | Low — stable load, policies should converge |
| `consolidate-when-diurnal` | 8 web apps + 10 microservices, diurnal traffic, scale-down at trough | High — off-peak creates consolidation opportunities |
| `consolidate-when-batch-heavy` | 6 batch jobs (exponential duration), low priority | Medium — batch completion creates empty nodes |
| `consolidate-when-mixed-priority` | High-priority web + low-priority batch + PDB-protected services | High — disruption cost varies dramatically per node |
| `consolidate-when-churn` | 50 pods with scale-up/scale-down oscillation | High — frequent state changes stress consolidation timing |

### 3.2 Threshold Sensitivity Analysis

**Goal:** Map the decision ratio threshold's effect on cost vs disruption.

Single scenario with 5 variants, each using `WhenCostJustifiesDisruption` at different thresholds:

| Variant | Threshold | Expected Behavior |
|---------|-----------|-------------------|
| `aggressive` | 0.5 | Consolidates aggressively — accepts small savings |
| `break-even` | 1.0 | Default — consolidates when savings >= disruption cost |
| `conservative` | 1.5 | Requires 50% more savings than disruption cost |
| `cautious` | 2.0 | Requires 2x savings — protects latency-sensitive workloads |
| `minimal-disruption` | 3.0 | Very conservative — only consolidates obvious waste |

Two workload profiles for threshold sensitivity:
1. **Heterogeneous mixed** — diverse pod sizes, priorities, PDBs (wide ratio distribution)
2. **Homogeneous web** — uniform pod sizes, same priority (narrow ratio distribution)

### 3.3 Latency-Sensitive vs Batch Divergence

**Goal:** Show scenarios where `WhenCostJustifiesDisruption` with high threshold protects latency-sensitive workloads while `WhenEmptyOrUnderutilized` disrupts them.

| Scenario | Setup | Key Metric |
|----------|-------|------------|
| `latency-sensitive-protection` | High-priority web apps with PDBs + low-priority batch, diurnal traffic | Pod eviction count for high-priority pods |
| `batch-aggressive-consolidation` | Pure batch workloads, exponential completion times | Cost savings from aggressive consolidation |

### 3.4 Cost vs Disruption Tradeoff Curve

**Goal:** Generate data for a Pareto frontier plot of cost savings vs disruption events.

Single scenario with 10 variants spanning threshold 0.25 to 5.0:
`[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 5.0]`

Workload: mixed (web + batch + microservices) with diurnal traffic pattern.
Metrics: `total_cost_per_hour` vs `pods_evicted` per variant.

### 3.5 Interaction with Existing Adversarial Scenarios

**Goal:** Re-run existing adversarial worst-cases with the new policies to check for regressions or amplified divergence.

Take the top 5 scheduling adversarial scenarios (`scenarios/adversarial/scheduling/worst_case_01-05.yaml`) and add `ConsolidateWhen` variants. This tests whether the new policy interacts badly with already-adversarial workload configurations.

### 3.6 Adversarial Search for ConsolidateWhen

**Goal:** Use Optuna TPE to find scenarios where the 3 policies diverge most.

New script: `scripts/find_adversarial_consolidate_when.py`

- Variant pair: `WhenEmptyOrUnderutilized` vs `WhenCostJustifiesDisruption` (threshold=1.0)
- Objectives: `cost_efficiency`, `availability`, `disruption_rate`, `consolidation_waste`
- Search space: same as existing adversarial search + `decision_ratio_threshold` ∈ [0.5, 3.0]
- Budget: 100 (start conservative, scale up)
- Output: `scenarios/adversarial/consolidate-when/worst_case_*.yaml`

---

## 4. Draft Scenario Files

Located in `scenarios/consolidate-when/`:

| File | Section | Variants |
|------|---------|----------|
| `policy-comparison-diurnal.yaml` | 3.1 | 3 policies |
| `policy-comparison-batch.yaml` | 3.1 | 3 policies |
| `policy-comparison-mixed-priority.yaml` | 3.1 | 3 policies |
| `threshold-sensitivity-heterogeneous.yaml` | 3.2 | 5 thresholds |
| `threshold-sensitivity-homogeneous.yaml` | 3.2 | 5 thresholds |
| `latency-vs-batch.yaml` | 3.3 | 3 policies × 2 thresholds |
| `cost-disruption-tradeoff.yaml` | 3.4 | 10 threshold points |
| `adversarial-interaction-01.yaml` | 3.5 | 3 policies on worst-case workload |

---

## 5. Success Criteria

1. **Policy divergence detected:** At least one scenario shows >10% cost difference between `WhenEmptyOrUnderutilized` and `WhenCostJustifiesDisruption`
2. **Threshold sensitivity visible:** Tradeoff curve shows monotonic decrease in disruption as threshold increases
3. **Latency protection demonstrated:** High-threshold variant shows fewer high-priority pod evictions
4. **No regressions:** Existing adversarial scenarios don't show worse behavior under new policies
5. **Adversarial search finds edge cases:** Optuna discovers at least 5 scenarios with combined divergence > 0.1

---

## 6. Implementation Order

1. **Schema changes** — Add `WhenCostJustifiesDisruption` policy + threshold to scenario YAML
2. **Decision ratio calculator** — Implement in `consolidation.rs`
3. **Metrics** — Add decision ratio tracking to metrics collector
4. **Variant support** — Wire through variant-level policy overrides
5. **Python bindings** — Expose new fields in `batch_run`
6. **Draft scenarios** — Create YAML files (this deliverable)
7. **Adversarial search script** — `find_adversarial_consolidate_when.py`
8. **Run experiments** — Execute scenarios, collect results
9. **Analysis** — Generate tradeoff curves and comparison reports
