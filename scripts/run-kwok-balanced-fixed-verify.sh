#!/usr/bin/env bash
# run-kwok-balanced-fixed-verify.sh — 3-variant KWOK verification with kp-d9x Balanced fix
# Variants: when-empty, balanced-k2, when-underutilized
# Sequence: 500→350→10 replicas × 2 deployments, 35min per variant, consolidateAfter=30s
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/kwok-balanced-fixed"
NAMESPACE="${NAMESPACE:-default}"
METRICS_INTERVAL="${METRICS_INTERVAL:-60}"
KIND_CONTEXT="${KIND_CONTEXT:-kind-kubesim}"

VARIANTS=(balanced-when-empty balanced-k2-fixed balanced-when-underutilized)
VARIANT_LABELS=(when-empty balanced-k2 when-underutilized)

log() { echo "[$(date -u +%H:%M:%S)] $*"; }

cleanup_variant() {
  log "Cleaning up..."
  kubectl --context "$KIND_CONTEXT" delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found 2>/dev/null || true
  sleep 5
  kubectl --context "$KIND_CONTEXT" delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl --context "$KIND_CONTEXT" delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 10
  local remaining
  remaining=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  if [ "$remaining" -gt 0 ]; then
    log "WARNING: $remaining nodes still present, waiting..."
    sleep 15
  fi
}

collect_timeseries() {
  local out_file="$1"
  > "$out_file"
  while true; do
    local ts nodes pods pending
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
    pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pending=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    echo "{\"ts\":\"$ts\",\"nodes\":$nodes,\"pods\":$pods,\"pending\":$pending}" >> "$out_file"
    sleep "$METRICS_INTERVAL"
  done
}

run_variant() {
  local template_name="$1"
  local label="$2"
  local template="$TEMPLATES_DIR/${template_name}.yaml"
  local variant_dir="$RESULTS_DIR/$label"

  [ -f "$template" ] || { log "ERROR: Template not found: $template"; return 1; }
  mkdir -p "$variant_dir"

  log "=== Running variant: $label (template: $template_name) ==="

  # 1. Apply NodePool
  log "Applying NodePool"
  kubectl --context "$KIND_CONTEXT" apply -f "$template"
  sleep 5

  # 2. Deploy workloads at 1 replica
  log "Deploying workloads"
  kubectl --context "$KIND_CONTEXT" apply -f "$MANIFESTS_DIR/deployment-a.yaml" -n "$NAMESPACE"
  kubectl --context "$KIND_CONTEXT" apply -f "$MANIFESTS_DIR/deployment-b.yaml" -n "$NAMESPACE"
  sleep 5

  # 3. Start timeseries collection
  collect_timeseries "$variant_dir/timeseries.jsonl" &
  local ts_pid=$!

  # 4. Scale up to 500
  log "t=0: Scaling to 500 replicas"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=500 -n "$NAMESPACE"

  # Wait for scheduling
  log "Waiting for pods to schedule..."
  local max_wait=180 waited=0
  while [ "$waited" -lt "$max_wait" ]; do
    local pending
    pending=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    if [ "$pending" -eq 0 ]; then break; fi
    sleep 5
    waited=$((waited + 5))
  done
  log "Scale-up complete (waited ${waited}s)"

  # Snapshot at peak
  local peak_nodes peak_pods
  peak_nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  peak_pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers --field-selector=status.phase=Running 2>/dev/null | wc -l | tr -d ' ')
  log "Peak: $peak_nodes nodes, $peak_pods running pods"

  # 5. Wait until t=15m then scale 500→350
  log "Waiting until t=15m for scale-down phase 1..."
  sleep 870  # ~14m30s (already spent ~30s on scheduling)

  log "t=15m: Scaling down to 350 replicas"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=350 -n "$NAMESPACE"

  # Capture 500→350 window metrics after consolidation settles
  sleep 120
  local mid_nodes mid_pods
  mid_nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  mid_pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers --field-selector=status.phase=Running 2>/dev/null | wc -l | tr -d ' ')
  log "Mid-point (500→350): $mid_nodes nodes, $mid_pods running pods"

  # Capture 500→350 window Karpenter logs
  kubectl --context "$KIND_CONTEXT" logs -n kube-system -l app.kubernetes.io/name=karpenter --since=5m \
    > "$variant_dir/karpenter-500-350.log" 2>/dev/null || true

  # 6. Wait until t=25m then scale 350→10
  sleep 480  # ~8m to reach t=25m

  log "t=25m: Scaling down to 10 replicas"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=10 -n "$NAMESPACE"

  # Wait for consolidation to settle
  log "Waiting 10m for consolidation (350→10 window)..."
  sleep 600

  # 7. Stop timeseries
  kill "$ts_pid" 2>/dev/null || true
  wait "$ts_pid" 2>/dev/null || true

  # 8. Collect full Karpenter logs
  log "Collecting Karpenter logs"
  kubectl --context "$KIND_CONTEXT" logs -n kube-system -l app.kubernetes.io/name=karpenter --since=40m \
    > "$variant_dir/karpenter-full.log" 2>/dev/null || true

  grep -E '(disrupting|consolidat|Balanced|Empty|Underutilized|disruption|consolidation_score)' \
    "$variant_dir/karpenter-full.log" > "$variant_dir/karpenter-consolidation.log" 2>/dev/null || true

  # 9. Count disruptions by path
  local evictions empty_decisions balanced_decisions underutil_decisions
  evictions=$(grep -c 'disrupting node' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || evictions=0
  empty_decisions=$(grep -c 'Empty/' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || empty_decisions=0
  balanced_decisions=$(grep -c 'Balanced/' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || balanced_decisions=0
  underutil_decisions=$(grep -c 'Underutilized/' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || underutil_decisions=0

  # Extract consolidation_score values (for balanced-k2)
  grep -o 'consolidation_score[^,}]*' "$variant_dir/karpenter-full.log" \
    > "$variant_dir/consolidation-scores.txt" 2>/dev/null || true

  # 10. Pod eviction count from events
  local pod_evictions
  pod_evictions=$(kubectl --context "$KIND_CONTEXT" get events -n "$NAMESPACE" --field-selector=reason=Evicted --no-headers 2>/dev/null | wc -l | tr -d ' ') || pod_evictions=0

  # 11. Final snapshot
  local final_nodes final_pods
  final_nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  final_pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  cat > "$variant_dir/summary.json" <<EOF
{
  "variant": "$label",
  "peak_node_count": $peak_nodes,
  "mid_node_count": $mid_nodes,
  "final_node_count": $final_nodes,
  "final_pod_count": $final_pods,
  "node_disruptions": $evictions,
  "pod_evictions": $pod_evictions,
  "empty_decisions": $empty_decisions,
  "balanced_decisions": $balanced_decisions,
  "underutil_decisions": $underutil_decisions,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

  log "=== $label: peak=$peak_nodes mid=$mid_nodes final=$final_nodes disruptions=$evictions empty=$empty_decisions balanced=$balanced_decisions underutil=$underutil_decisions ==="

  cleanup_variant
}

main() {
  log "Starting 3-variant KWOK verification with kp-d9x Balanced fix"
  log "Cluster context: $KIND_CONTEXT"
  mkdir -p "$RESULTS_DIR"

  kubectl --context "$KIND_CONTEXT" cluster-info > /dev/null 2>&1 || {
    log "ERROR: Cannot reach cluster $KIND_CONTEXT"
    exit 1
  }

  cleanup_variant

  local failed=0
  for i in "${!VARIANTS[@]}"; do
    if ! run_variant "${VARIANTS[$i]}" "${VARIANT_LABELS[$i]}"; then
      log "ERROR: Variant ${VARIANT_LABELS[$i]} failed"
      failed=$((failed + 1))
    fi
  done

  # Summary
  log ""
  log "=== KWOK Balanced-Fixed Verification Summary ==="
  log ""
  printf "%-20s %6s %6s %6s %6s %8s %8s %8s\n" "Variant" "Peak" "Mid" "Final" "Disrupt" "Empty/" "Balanced/" "Underutil/"
  printf "%-20s %6s %6s %6s %6s %8s %8s %8s\n" "-------" "----" "---" "-----" "-------" "------" "---------" "----------"
  for label in "${VARIANT_LABELS[@]}"; do
    local summary="$RESULTS_DIR/$label/summary.json"
    if [ -f "$summary" ]; then
      python3 -c "
import json
s = json.load(open('$summary'))
print(f\"{s['variant']:<20s} {s['peak_node_count']:>6} {s['mid_node_count']:>6} {s['final_node_count']:>6} {s['node_disruptions']:>6} {s['empty_decisions']:>8} {s['balanced_decisions']:>8} {s['underutil_decisions']:>8}\")
" 2>/dev/null || log "  $label: (parse error)"
    fi
  done

  log ""
  log "Sim predictions for comparison:"
  log "  when-empty:        disruptions=0,   final_nodes=3.6"
  log "  balanced-k2:       disruptions=140, final_nodes=1.0"
  log "  when-underutilized: disruptions=516, final_nodes=1.0"
  log ""
  log "Results in: $RESULTS_DIR"
  [ "$failed" -eq 0 ] || exit 1
}

main "$@"
