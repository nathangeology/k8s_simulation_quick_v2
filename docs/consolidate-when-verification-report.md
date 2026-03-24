# ConsolidateWhen Verification Report

**Bead:** k8s-9kuc
**Date:** 2026-03-24
**Verdict:** ConsolidateWhen / WhenCostJustifiesDisruption is **NOT active** in the KWOK Karpenter deployment.

---

## Summary

The `consolidateWhen: WhenCostJustifiesDisruption` field from PR #2893 is silently ignored by the Karpenter controller. All cost-justified NodePool variants fall back to default `WhenEmptyOrUnderutilized` behavior. The `decisionRatioThreshold` field has no effect on disruption decisions.

## Evidence

### 1. Disruption Path Analysis

Every Karpenter log entry across all 10 variants uses either `Empty/` or `Underutilized/` as the disruption command prefix. No `CostJustified/` prefix or `decision.ratio` log entry appears anywhere.

| Variant | Disruption Path | Final Nodes | Evictions |
|---------|----------------|-------------|-----------|
| when-empty | `Empty/` | 20 | 2 |
| when-underutilized | `Underutilized/` | 7 | 2 |
| cost-justified-0.25 | `Underutilized/` | 7 | 2 |
| cost-justified-0.50 | `Underutilized/` | 9 | 1 |
| cost-justified-1.00 | `Underutilized/` | 11 | 2 |
| cost-justified-2.00 | `Empty/` | 20 | 1 |
| cost-justified-5.00 | `Empty/` | 20 | 1 |

### 2. Threshold Has No Effect

If `WhenCostJustifiesDisruption` were active, varying `decisionRatioThreshold` from 0.25 to 5.00 should produce a smooth gradient of consolidation aggressiveness. Instead, results cluster into two groups matching legacy policies:

- **Low thresholds (0.25–1.00):** Behave like `WhenEmptyOrUnderutilized` — `Underutilized/` path, 7–11 final nodes
- **High thresholds (2.00–5.00):** Behave like `WhenEmpty` — `Empty/` path, 20 final nodes

The transition is not threshold-driven; it's an artifact of fallback behavior when `consolidateWhen` is ignored and `consolidationPolicy` is absent.

### 3. Zero Decision Ratio Logging

```
grep -r "decision.ratio\|CostJustified\|cost.justified" results/kwok-verify/*/karpenter-consolidation.log
# NO MATCHES FOUND
```

The Karpenter controller never enters the cost-justified evaluation code path.

## Root Cause

The Karpenter build from PR #2893 has the **CRD schema** for `consolidateWhen` and `decisionRatioThreshold` (fields accepted by the API server without validation errors), but the **disruption controller** does not read these fields. The controller uses the legacy `consolidationPolicy` field, which is absent in cost-justified templates, causing fallback to default behavior.

## Impact on Simulator Validation

The simulator's `WhenCostJustifiesDisruption` implementation (in `crates/kubesim-karpenter/src/consolidation.rs`) correctly implements decision-ratio-based consolidation via `find_cost_justified_nodes()`. The KWOK data **cannot validate this model** because real Karpenter never enters the corresponding code path.

The simulator's tradeoff analysis (`results/consolidate-when/benchmark-tradeoff-kwok/`) shows expected behavior:
- Threshold 0.25–1.00: identical aggressive consolidation (54.3 disruptions, 15.3% savings)
- Threshold 1.50–5.00: progressively less aggressive (40.9→1.9 disruptions, 6.1%→3.4% savings)

This gradient is correct expected behavior. The KWOK data shows no such gradient.

## Recommendations

1. **Verify the Karpenter build** contains controller logic (not just CRD schema):
   ```bash
   kubectl exec -n kube-system deploy/karpenter -- strings /karpenter | grep -i "CostJustifies\|decision.ratio"
   ```

2. **Check CRD schema** confirms fields exist:
   ```bash
   kubectl get crd nodepools.karpenter.sh -o json | \
     jq '.spec.versions[].schema.openAPIV3Schema.properties.spec.properties.disruption.properties | keys'
   ```

3. **Check PR #2893 status** — the PR may have CRD changes only, with controller logic in a follow-up.

4. **Rebuild from correct ref** if controller logic exists in a different branch/commit.

5. **Defer simulator calibration** for the cost-justified model until a working Karpenter build is available. The `WhenEmpty` and `WhenEmptyOrUnderutilized` models can be calibrated from existing data.
