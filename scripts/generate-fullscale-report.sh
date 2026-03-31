#!/usr/bin/env bash
# generate-fullscale-report.sh — Compare fullscale KWOK results against simulator predictions
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$REPO_ROOT/results/kwok-verify-fullscale"
SIM_FILE="$REPO_ROOT/results/consolidate-when/benchmark-tradeoff-kwok/results.json"
REPORT="$RESULTS_DIR/comparison-report.md"

die() { echo "FATAL: $*" >&2; exit 1; }

[ -d "$RESULTS_DIR" ] || die "Results dir not found: $RESULTS_DIR"

# Map variant dir names to simulator keys
declare -A SIM_KEYS=(
  [when-empty]="WhenEmpty"
  [when-underutilized]="WhenEmptyOrUnderutilized"
  [cost-justified-1.00]="CostJustified-1.00"
  [cost-justified-5.00]="CostJustified-5.00"
)

VARIANTS=(when-empty when-underutilized cost-justified-1.00 cost-justified-5.00)

# Read KWOK results
declare -A KWOK_EVICTIONS KWOK_NODES
for v in "${VARIANTS[@]}"; do
  sf="$RESULTS_DIR/$v/summary.json"
  if [ -f "$sf" ]; then
    KWOK_EVICTIONS[$v]=$(python3 -c "import json; print(json.load(open('$sf'))['pods_evicted'])")
    KWOK_NODES[$v]=$(python3 -c "import json; print(json.load(open('$sf'))['final_node_count'])")
  else
    KWOK_EVICTIONS[$v]="N/A"
    KWOK_NODES[$v]="N/A"
  fi
done

# Read simulator predictions
declare -A SIM_DISRUPTIONS
if [ -f "$SIM_FILE" ]; then
  for v in "${VARIANTS[@]}"; do
    sk="${SIM_KEYS[$v]}"
    SIM_DISRUPTIONS[$v]=$(python3 -c "import json; d=json.load(open('$SIM_FILE')); print(d.get('$sk',{}).get('disruption_count','N/A'))")
  done
else
  for v in "${VARIANTS[@]}"; do SIM_DISRUPTIONS[$v]="N/A"; done
fi

# Check acceptance criteria
we_ev="${KWOK_EVICTIONS[when-empty]}"
wu_ev="${KWOK_EVICTIONS[when-underutilized]}"
cj1_ev="${KWOK_EVICTIONS[cost-justified-1.00]}"
cj5_ev="${KWOK_EVICTIONS[cost-justified-5.00]}"

ordering_pass="❌"
if [[ "$we_ev" != "N/A" && "$wu_ev" != "N/A" && "$cj1_ev" != "N/A" && "$cj5_ev" != "N/A" ]]; then
  if [ "$wu_ev" -gt "$cj1_ev" ] && [ "$cj1_ev" -gt "$cj5_ev" ] && [ "$cj5_ev" -ge "$we_ev" ]; then
    ordering_pass="✅"
  fi
fi

we_zero="❌"
[[ "$we_ev" == "0" ]] && we_zero="✅"

cat > "$REPORT" <<EOF
# Full-Scale KWOK Verification Report

Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Parameters: 500 replicas, 35-min sequence, consolidateAfter=30s, 60s metrics

## Acceptance Criteria

| Criterion | Pass | Detail |
|-----------|------|--------|
| WhenEmpty zero-disruption | $we_zero | evictions=$we_ev |
| Disruption ordering: wu > cj-1.00 > cj-5.00 ≥ we | $ordering_pass | $wu_ev > $cj1_ev > $cj5_ev ≥ $we_ev |

## Per-Variant Results

| Variant | KWOK Evictions | Sim Disruptions | KWOK Final Nodes |
|---------|---------------|-----------------|------------------|
EOF

for v in "${VARIANTS[@]}"; do
  echo "| $v | ${KWOK_EVICTIONS[$v]} | ${SIM_DISRUPTIONS[$v]} | ${KWOK_NODES[$v]} |" >> "$REPORT"
done

cat >> "$REPORT" <<'EOF'

## Simulator Predictions (reference)

From `results/consolidate-when/benchmark-tradeoff-kwok/results.json`:
- WhenEmpty: 0.0 disruptions (baseline)
- WhenEmptyOrUnderutilized: 438.2 disruptions (most aggressive)
- CostJustified-1.00: 54.3 disruptions (knee point)
- CostJustified-5.00: 1.95 disruptions (conservative)

Expected ordering: wu(438) >> cj-1.00(54) >> cj-5.00(2) >> we(0)

## Node Count Gradient

If the KWOK results show differentiated final node counts across variants,
the cost-justified threshold is controlling consolidation aggressiveness
as designed. Lower thresholds → more consolidation → fewer final nodes.
EOF

echo "Report written to: $REPORT"
