#!/usr/bin/env bash
# generate-pod-deletion-cost-report.sh — Compare pod-deletion-cost KWOK results
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$REPO_ROOT/results/pod-deletion-cost-verify"
REPORT="$RESULTS_DIR/comparison-report.md"

die() { echo "FATAL: $*" >&2; exit 1; }

[ -d "$RESULTS_DIR" ] || die "Results dir not found: $RESULTS_DIR"

VARIANTS=(no-cost low-cost mid-cost high-cost mixed-cost)

declare -A KWOK_EVICTIONS KWOK_NODES KWOK_CJ KWOK_DR COST_A_VAL COST_B_VAL
COST_A_VAL=([no-cost]="none" [low-cost]="1" [mid-cost]="50" [high-cost]="1000" [mixed-cost]="1000")
COST_B_VAL=([no-cost]="none" [low-cost]="none" [mid-cost]="none" [high-cost]="none" [mixed-cost]="1")

for v in "${VARIANTS[@]}"; do
  sf="$RESULTS_DIR/$v/summary.json"
  if [ -f "$sf" ]; then
    KWOK_EVICTIONS[$v]=$(python3 -c "import json; print(json.load(open('$sf'))['pods_evicted'])")
    KWOK_NODES[$v]=$(python3 -c "import json; print(json.load(open('$sf'))['final_node_count'])")
    KWOK_CJ[$v]=$(python3 -c "import json; print(json.load(open('$sf'))['cost_justified_path'])")
    KWOK_DR[$v]=$(python3 -c "import json; print(json.load(open('$sf'))['decision_ratio_entries'])")
  else
    KWOK_EVICTIONS[$v]="N/A"
    KWOK_NODES[$v]="N/A"
    KWOK_CJ[$v]="N/A"
    KWOK_DR[$v]="N/A"
  fi
done

# Check acceptance criteria
nc_ev="${KWOK_EVICTIONS[no-cost]}"
lc_ev="${KWOK_EVICTIONS[low-cost]}"
hc_ev="${KWOK_EVICTIONS[high-cost]}"
mx_ev="${KWOK_EVICTIONS[mixed-cost]}"

# High-cost should have fewer evictions than no-cost (protection works)
gradient_pass="❌"
if [[ "$nc_ev" != "N/A" && "$hc_ev" != "N/A" ]]; then
  if [ "$hc_ev" -le "$nc_ev" ]; then
    gradient_pass="✅"
  fi
fi

# CostJustified path should activate (decision.ratio entries > 0)
cj_active="❌"
if [[ "${KWOK_DR[no-cost]}" != "N/A" && "${KWOK_DR[no-cost]}" -gt 0 ]]; then
  cj_active="✅"
fi

cat > "$REPORT" <<EOF
# Pod-Deletion-Cost Disruption Scoring Verification Report

Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Parameters: 500 replicas, 35-min sequence, consolidateAfter=30s, k=2 (threshold=0.5)

## Acceptance Criteria

| Criterion | Pass | Detail |
|-----------|------|--------|
| CostJustified path activates | $cj_active | decision.ratio entries: ${KWOK_DR[no-cost]} |
| High-cost ≤ no-cost evictions | $gradient_pass | high=$hc_ev ≤ no=$nc_ev |

## Per-Variant Results

| Variant | Cost A | Cost B | Evictions | CJ Path | DR Entries | Final Nodes |
|---------|--------|--------|-----------|---------|------------|-------------|
EOF

for v in "${VARIANTS[@]}"; do
  echo "| $v | ${COST_A_VAL[$v]} | ${COST_B_VAL[$v]} | ${KWOK_EVICTIONS[$v]} | ${KWOK_CJ[$v]} | ${KWOK_DR[$v]} | ${KWOK_NODES[$v]} |" >> "$REPORT"
done

cat >> "$REPORT" <<'EOF'

## Expected Behavior

### Scoring Formula
`score = savings_fraction / disruption_fraction`
- `disruption_fraction = move_disruption_cost / nodepool_total_disruption_cost`
- Per-pod disruption cost from `controller.kubernetes.io/pod-deletion-cost` (default 1.0)

### Expected Gradient
1. **no-cost**: All pods cost=1.0. Uniform scoring. Baseline eviction count.
2. **low-cost**: deployment-a cost=1 (same as default). Similar to no-cost.
3. **mid-cost**: deployment-a cost=50. Nodes with deployment-a pods have higher
   disruption_fraction → lower decision_ratio → fewer evictions of those nodes.
4. **high-cost**: deployment-a cost=1000. Strong protection. Consolidation strongly
   prefers evicting nodes with only deployment-b pods (cost=1).
5. **mixed-cost**: deployment-a=1000, deployment-b=1. Clear differentiation.
   Nodes with deployment-b pods have much lower disruption_fraction → higher
   decision_ratio → evicted first.

### Key Signal
If evictions decrease as deletion_cost increases (no-cost ≥ low ≥ mid ≥ high),
the scoring formula correctly weights pod-deletion-cost in consolidation decisions.
EOF

echo "Report written to: $REPORT"
