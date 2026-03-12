# Disruption Budget Audit

**Date:** 2026-03-12
**Bead:** k8s-bnr

---

## 1. Current Implementation Status

### 1.1 How is `max_disrupted_pct` used in consolidation?

**File:** `crates/kubesim-karpenter/src/consolidation.rs`

The `disruption_budget()` function computes the max disrupted node count:

```rust
pub fn disruption_budget(pool: &NodePool, total_nodes: u32) -> u32 {
    ((total_nodes as u64 * pool.max_disrupted_pct as u64) / 100).max(1) as u32
}
```

This is called in `ConsolidationHandler::handle()` before `evaluate_versioned()`. The
resulting `max_disrupted` cap is enforced across all actions in a single evaluation pass —
empty node terminations, drain-and-terminate, and replace actions all count against it.

### 1.2 Is `max_disrupted_pct` configurable per-pool in scenario YAML?

**No. It is hardcoded to 10%.**

In `crates/kubesim-py/src/lib.rs` (line 349), the `NodePool` is constructed with:

```rust
max_disrupted_pct: 10,
```

The `NodePoolDef` in `scenario.rs` has no `max_disrupted_pct` field. The `nodepool.rs`
struct has a `default_disruption_pct()` serde default of 10, but this is never populated
from scenario YAML because the scenario-to-pool conversion in `lib.rs` bypasses serde
deserialization and hardcodes the value.

**Gap:** Users cannot configure disruption budget percentage per pool in scenario YAML.

### 1.3 Does it respect the budget across multiple consolidation actions in the same loop?

**Yes, within a single `evaluate_versioned()` call.** The function tracks `total_used`
across empty, underutilized, and replace actions, breaking when `total_used >= max_disrupted`.

**No, across consecutive loops.** The budget resets every consolidation loop (default 30s
interval). There is no accumulation or cooldown tracking between loops. If 10 nodes are
disrupted in loop N, loop N+1 can disrupt another 10 (assuming the pool still has nodes).

This matches upstream Karpenter behavior — budgets are per-evaluation, not cumulative.

### 1.4 Do we support both percentage and absolute count budgets?

**No. Percentage only.**

Upstream Karpenter `Budget.Nodes` accepts either a percentage string (`"10%"`) or an
absolute integer (`"5"`). Our `DisruptionBudgetConfig` only has `max_percent: u32`.

The `DisruptionBudgetConfig` struct in `version.rs` has no field for absolute count.

### 1.5 Do we model schedule-based disruption budget overrides?

**Partially. The data model exists but is not evaluated.**

`DisruptionBudgetConfig` has a `schedule: Option<String>` field for cron expressions,
but `evaluate_versioned()` never checks it. The schedule field is always `None` in the
default `VersionProfile::new()` constructors, and there is no cron parsing or
time-gated evaluation logic anywhere in the codebase.

### 1.6 Does the budget reset between consolidation loops?

**Yes.** Each call to `evaluate_versioned()` starts with `total_used = 0`. There is no
state carried between invocations. This is correct behavior — upstream Karpenter also
evaluates budgets per disruption cycle.

### 1.7 How does the budget interact with `do_not_disrupt` pods?

**Correctly.** `find_underutilized_nodes()` and `find_replace_candidates()` both filter
out nodes containing `do_not_disrupt` pods via `node_has_do_not_disrupt()`. These nodes
are excluded from candidacy entirely — they don't consume budget.

The `do_not_disrupt` field exists on `Pod` in `kubesim-core` and is checked in
consolidation. However, there is no `do_not_disrupt` annotation on **nodes** (only pods).
Upstream Karpenter also supports `karpenter.sh/do-not-disrupt` on Node objects.

### 1.8 Are there scenarios that test budget limits?

**No dedicated budget-limit scenarios exist.** The existing tests in `consolidation.rs`
cover:
- `disruption_budget_limits_actions` — 3 empty nodes, budget of 1, asserts only 1 action
- `disruption_budget_calculation` — unit test for the percentage math

No scenario YAML exercises budget limits at scale (e.g., 100 nodes with 10% budget).

---

## 2. Gaps vs Upstream Karpenter

| Feature | Upstream Karpenter v1.x | kubesim | Gap |
|---------|------------------------|---------|-----|
| Percentage-based budget | ✅ `nodes: "10%"` | ✅ `max_disrupted_pct` | None |
| Absolute count budget | ✅ `nodes: "5"` | ❌ | Missing |
| Per-pool budget config in YAML | ✅ per-NodePool `spec.disruption.budgets[]` | ❌ hardcoded 10% | Missing |
| Per-reason budgets | ✅ `budgets[].reasons: [Underutilized, Empty, Drifted]` | ⚠️ Data model exists, partially wired in `evaluate_versioned` | Partially implemented |
| Schedule-based overrides | ✅ `budgets[].schedule` + `duration` (cron windows) | ❌ Field exists but never evaluated | Missing |
| Budget resets per cycle | ✅ | ✅ | None |
| `do_not_disrupt` on pods | ✅ | ✅ | None |
| `do_not_disrupt` on nodes | ✅ | ❌ | Missing |
| Multi-node consolidation respects budget | ✅ | ✅ (within single eval) | None |
| `nodes: "0"` (block all disruption) | ✅ | ❌ (min 1 enforced) | Missing |

---

## 3. Recommendations

### P1: Make `max_disrupted_pct` configurable from scenario YAML

Add a `disruption_budget` field to `NodePoolDef` or `KarpenterConfig` in `scenario.rs`,
and wire it through to `NodePool.max_disrupted_pct` in `lib.rs` instead of hardcoding 10.

**Complexity:** Low. Add one field, change one line in pool construction.

### P2: Support absolute count budgets

Add `max_nodes: Option<u32>` to `DisruptionBudgetConfig`. In `disruption_budget()`,
return `min(percentage_result, absolute_cap)` when both are set, or just the absolute
cap when only count is specified.

**Complexity:** Low.

### P3: Support `nodes: "0"` (block all disruption)

Remove the `.max(1)` floor in `disruption_budget()` when the configured budget is
explicitly 0. This requires distinguishing "not configured" (default 10%) from
"explicitly set to 0%".

**Complexity:** Low.

### P4: Wire schedule-based budget overrides

Implement cron parsing for `DisruptionBudgetConfig.schedule` and evaluate it against
simulation time in `evaluate_versioned()`. Only apply a budget entry when the current
sim time falls within its schedule window.

**Complexity:** Medium. Requires cron parsing (use `cron` crate) and sim-time awareness.

### P5: Add `do_not_disrupt` on nodes

Add a `do_not_disrupt: bool` field to `Node` in `kubesim-core`. Check it in
consolidation candidate filtering alongside the pod-level check.

**Complexity:** Low.

---

## 4. Example Scenario Configs Exercising Budget Limits

### 4.1 Large cluster with tight budget (10% of 100 = max 10 disrupted)

```yaml
study:
  name: disruption-budget-tight
  runs: 50
  time_mode: wall_clock
  cluster:
    node_pools:
      - instance_types: [m5.xlarge, m5.2xlarge]
        min_nodes: 100
        max_nodes: 100
        karpenter:
          consolidation:
            policy: WhenUnderutilized
          disruption_budget: "10%"    # proposed field
  workloads:
    - type: web_app
      count: 20
      replicas:
        fixed: 3
      cpu_request:
        dist: uniform
        min: "250m"
        max: "500m"
      memory_request:
        dist: uniform
        min: "256Mi"
        max: "512Mi"
      scale_down:
        - {at: "6h", reduce_by: 2}   # triggers consolidation of ~40 nodes
  traffic_pattern:
    type: diurnal
    peak_multiplier: 3.0
    duration: "24h"
```

### 4.2 Zero-budget maintenance window (block disruption during business hours)

```yaml
study:
  name: disruption-budget-scheduled
  runs: 50
  time_mode: wall_clock
  cluster:
    node_pools:
      - instance_types: [m5.xlarge, c5.xlarge]
        min_nodes: 50
        max_nodes: 50
        karpenter:
          consolidation:
            policy: WhenUnderutilized
          disruption_budgets:           # proposed field (v1.x style)
            - nodes: "10%"
            - nodes: "0"
              schedule: "0 9 * * mon-fri"
              duration: "8h"
  workloads:
    - type: saas_microservice
      count: 15
      replicas:
        fixed: 5
      cpu_request:
        dist: uniform
        min: "100m"
        max: "300m"
      memory_request:
        dist: uniform
        min: "128Mi"
        max: "256Mi"
```

### 4.3 Absolute count budget (max 5 nodes disrupted regardless of cluster size)

```yaml
study:
  name: disruption-budget-absolute
  runs: 50
  time_mode: wall_clock
  cluster:
    node_pools:
      - instance_types: [m5.xlarge]
        min_nodes: 200
        max_nodes: 200
        karpenter:
          consolidation:
            policy: WhenUnderutilized
          disruption_budgets:
            - nodes: "5"              # absolute count, proposed
  workloads:
    - type: batch_job
      count: 50
      cpu_request:
        dist: uniform
        min: "500m"
        max: "2000m"
      memory_request:
        dist: uniform
        min: "512Mi"
        max: "2Gi"
      duration:
        dist: exponential
        mean: "1h"
```
