# Karpenter Version Comparison: v0.35 vs v1.x

## Overview

This document describes the key behavioral differences between Karpenter v0.35
(pre-GA) and v1.x (GA), and how KubeSim models them via a strategy-based
version abstraction. The goal is to enable comparison simulations that show how
the same workload behaves under different Karpenter versions.

## Key Behavioral Differences

### 1. Consolidation Algorithm

| Behavior | v0.35 | v1.x |
|----------|-------|------|
| Empty node removal | Yes | Yes |
| Single-node delete | Yes (WhenUnderutilized) | Yes |
| Multi-node replacement | No | Yes — replace N underutilized nodes with fewer |
| Single-node replacement | No | Yes — swap to cheaper instance type |
| Combined policy | No (WhenEmpty OR WhenUnderutilized) | WhenEmptyOrUnderutilized option |

v0.35 consolidation is strictly subtractive: it can only remove nodes whose pods
fit elsewhere. v1.x adds replacement consolidation — it can terminate a node and
simultaneously launch a cheaper one, or consolidate multiple nodes into fewer.

**Simulation impact:** v1.x achieves lower steady-state cost because it can
right-size nodes, not just remove empty/underutilized ones.

### 2. Provisioning API

| Aspect | v0.35 | v1.x |
|--------|-------|------|
| CRD | `Provisioner` (single resource) | `NodePool` + `EC2NodeClass` (split) |
| Instance selection | Basic cheapest-fit | Improved bin-packing with first-fit-decreasing |
| Weight-based priority | No | Yes — NodePool `weight` field for priority ordering |

The API split in v1.x separates scheduling constraints (NodePool) from cloud
provider details (EC2NodeClass). For simulation purposes, the behavioral
difference is minor — the provisioning algorithm is similar. The main simulation
difference is that v1.x's improved bin-packing can pack more pods per node.

### 3. Disruption Budgets

| Feature | v0.35 | v1.x |
|---------|-------|------|
| Percentage-based limit | Yes | Yes |
| Per-reason budgets | No | Yes (Underutilized, Empty, Drifted) |
| Schedule windows | No | Yes (cron-based) |
| Duration limits | No | Yes |

v0.35 has a single disruption percentage applied uniformly. v1.x allows
fine-grained control: different budgets for different disruption reasons, with
time-window restrictions (e.g., "only consolidate during business hours").

**Simulation impact:** v1.x can be configured to be more aggressive during
off-peak and conservative during peak, leading to different cost/disruption
tradeoff curves.

### 4. Drift Detection

| Feature | v0.35 | v1.x |
|---------|-------|------|
| AMI/instance type drift | Yes | Yes |
| NodePool spec hash drift | No | Yes — any spec change triggers drift |
| Label/taint drift | No | Yes |
| Requirement drift | No | Yes |

v0.35 only detects drift when a node's instance type is no longer in the
Provisioner's allowed list (typically AMI updates). v1.x computes a hash of the
full NodePool spec and detects drift when any field changes — labels, taints,
requirements, etc.

**Simulation impact:** v1.x triggers more node replacements after config changes,
leading to higher short-term disruption but faster convergence to desired state.

### 5. Spot Instance Handling

| Feature | v0.35 | v1.x |
|---------|-------|------|
| ITN handling | Yes | Yes |
| Capacity type fallback | Limited | Yes — spot → on-demand fallback |
| Consolidation awareness | No | Yes — considers capacity type in consolidation |

Both versions handle spot interruption notifications (ITN). v1.x adds smarter
fallback behavior and considers capacity type during consolidation decisions.

## Design: Version Abstraction in KubeSim

### Strategy Pattern via `VersionProfile`

Rather than branching on version throughout the codebase, we use a
`VersionProfile` struct that resolves version-specific behavior at construction
time. Handlers receive the profile and use its fields to select strategies.

```rust
// Select version at simulation setup
let profile = VersionProfile::new(KarpenterVersion::V0_35);

// Pass to handlers — they use profile fields, not version checks
let consolidation = ConsolidationHandler::new(pool, policy)
    .with_version(profile.clone());
let drift = DriftHandler::new(pool, config)
    .with_version(profile);
```

### `VersionProfile` Fields

| Field | v0.35 | v1.x | Effect |
|-------|-------|------|--------|
| `consolidation_strategy` | `SingleNode` | `MultiNode` | Controls consolidation algorithm |
| `hash_based_drift` | `false` | `true` | Enables label/taint drift detection |
| `replace_consolidation` | `false` | `true` | Enables node replacement (not just deletion) |
| `budgets[].reasons` | empty | populated | Per-reason disruption budgets |
| `budgets[].schedule` | `None` | optional cron | Time-windowed disruption |

### Why Strategy Pattern (Not Trait Objects)

We considered three approaches:

1. **Feature flags**: Simple booleans. Too flat — doesn't capture version
   semantics or ensure consistent combinations.

2. **Trait objects**: `Box<dyn ConsolidationPolicy>` per version. Maximum
   flexibility but over-engineered for two known versions with mostly shared
   logic.

3. **Strategy enum + profile** (chosen): `KarpenterVersion` enum resolves to a
   `VersionProfile` with concrete strategy selections. Handlers branch on
   strategy fields. Minimal code duplication, easy to add v1.1/v2 later.

The strategy pattern wins because the versions share 90% of their logic. The
differences are in thresholds and algorithm selection, not fundamentally
different architectures.

## Running Comparison Simulations

To compare v0.35 vs v1.x behavior on the same workload:

```rust
use kubesim_karpenter::{KarpenterVersion, VersionProfile};

// Set up two simulation variants
let v035 = VersionProfile::new(KarpenterVersion::V0_35);
let v1   = VersionProfile::new(KarpenterVersion::V1);

// Create handlers with each profile, run same scenario, compare metrics
```

In scenario YAML (future integration):

```yaml
study:
  name: karpenter-version-comparison
  variants:
    - name: v0_35
      karpenter:
        version: v0.35
        consolidation: {policy: WhenUnderutilized}
    - name: v1
      karpenter:
        version: v1
        consolidation: {policy: WhenUnderutilized}
  metrics:
    compare: [total_cost, disruption_count, time_to_consolidate, node_count_over_time]
```

## Files Changed

- `crates/kubesim-karpenter/src/version.rs` — New: `KarpenterVersion`,
  `VersionProfile`, strategy enums, disruption budget config
- `crates/kubesim-karpenter/src/consolidation.rs` — Version-aware consolidation
  with `evaluate_versioned()`, `Replace` action variant
- `crates/kubesim-karpenter/src/drift.rs` — Version-aware drift detection
  (AMI-only vs hash-based)
- `crates/kubesim-karpenter/src/handler.rs` — `VersionProfile` on
  `ProvisioningHandler`
- `crates/kubesim-karpenter/src/lib.rs` — Re-exports for version module

## Future Work

- Multi-node consolidation implementation (currently strategy is wired but
  algorithm falls back to single-node)
- Single-node replacement consolidation (Replace action emitted but selection
  logic not yet integrated with EC2 catalog)
- Disruption budget schedule windows (cron parsing + time-gated evaluation)
- Scenario YAML integration for `karpenter.version` field
- Validation against real Karpenter behavior via Tier 2 (KWOK) comparison
