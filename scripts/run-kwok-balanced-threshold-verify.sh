#!/usr/bin/env bash
# run-kwok-balanced-threshold-verify.sh — Run 5 balanced consolidation variants on KIND+KWOK
# Uses default RS spreading heuristic (no deletion cost annotations).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/balanced-threshold-verify"
NAMESPACE="${NAMESPACE:-default}"
METRICS_INTERVAL="${METRICS_INTERVAL:-15}"
KIND_CONTEXT="${KIND_CONTEXT:-kind-kubesim}"

VARIANTS=(
  balanced-when-empty
  balanced-k1
  balanced-k2
  balanced-k4
  balanced-when-underutilized
)

VARIANT_LABELS=(
  when-empty
  balanced-k1
  balanced-k2
  balanced-k4
  when-underutilized
)

log() { echo "[$(date -u +%H:%M:%S)] $*"; }

cleanup_variant() {
  log "Cleaning up..."
  kubectl --context "$KIND_CONTEXT" delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found 2>/dev/null || true
  # Wait for pods to terminate
  sleep 5
  kubectl --context "$KIND_CONTEXT" delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl --context "$KIND_CONTEXT" delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 10
  # Verify clean state
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
  log "Scaling to 500 replicas"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=500 -n "$NAMESPACE"

  # Wait for all pods to be running
  log "Waiting for pods to schedule..."
  local max_wait=120
  local waited=0
  while [ "$waited" -lt "$max_wait" ]; do
    local pending
    pending=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    if [ "$pending" -eq 0 ]; then
      break
    fi
    sleep 5
    waited=$((waited + 5))
  done
  log "Scale-up complete (waited ${waited}s)"
  sleep 10

  # 5. Scale down 500→350
  log "Scaling down to 350 replicas (500→350)"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=350 -n "$NAMESPACE"

  # Wait for consolidation to act on partially-filled nodes
  log "Waiting 90s for consolidation (500→350 window)..."
  sleep 90

  # Capture mid-point snapshot
  local mid_nodes mid_pods
  mid_nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  mid_pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers --field-selector=status.phase=Running 2>/dev/null | wc -l | tr -d ' ')
  log "Mid-point: $mid_nodes nodes, $mid_pods running pods"

  # 6. Scale down 350→10
  log "Scaling down to 10 replicas (350→10)"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=10 -n "$NAMESPACE"

  # Wait for consolidation
  log "Waiting 90s for consolidation (350→10 window)..."
  sleep 90

  # 7. Stop timeseries
  kill "$ts_pid" 2>/dev/null || true
  wait "$ts_pid" 2>/dev/null || true

  # 8. Collect Karpenter logs
  log "Collecting Karpenter logs"
  kubectl --context "$KIND_CONTEXT" logs -n kube-system -l app.kubernetes.io/name=karpenter --since=10m \
    > "$variant_dir/karpenter-full.log" 2>/dev/null || true

  # Extract consolidation-relevant lines
  grep -E '(disrupting|consolidat|decision.ratio|CostJustified|Balanced|Empty|Underutilized|disruption)' \
    "$variant_dir/karpenter-full.log" > "$variant_dir/karpenter-consolidation.log" 2>/dev/null || true

  # 9. Count disruptions from logs
  local evictions
  evictions=$(grep -c 'disrupting node' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || evictions=0

  # Count by path
  local empty_decisions cj_decisions underutil_decisions
  empty_decisions=$(grep -c 'Empty/' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || empty_decisions=0
  cj_decisions=$(grep -c -E '(CostJustified/|Balanced/)' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || cj_decisions=0
  underutil_decisions=$(grep -c 'Underutilized/' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || underutil_decisions=0

  # 10. Final snapshot
  local final_nodes final_pods
  final_nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  final_pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  cat > "$variant_dir/summary.json" <<EOF
{
  "variant": "$label",
  "final_node_count": $final_nodes,
  "mid_node_count": $mid_nodes,
  "final_pod_count": $final_pods,
  "evictions": $evictions,
  "empty_decisions": $empty_decisions,
  "cj_decisions": $cj_decisions,
  "underutil_decisions": $underutil_decisions,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

  log "=== $label: final=$final_nodes nodes, mid=$mid_nodes nodes, evictions=$evictions, empty=$empty_decisions, CJ=$cj_decisions, underutil=$underutil_decisions ==="

  # 11. Cleanup
  cleanup_variant
}

# --- Main ---
main() {
  log "Starting balanced threshold gradient KWOK verification"
  log "Cluster context: $KIND_CONTEXT"
  mkdir -p "$RESULTS_DIR"

  # Verify cluster is reachable
  kubectl --context "$KIND_CONTEXT" cluster-info > /dev/null 2>&1 || {
    log "ERROR: Cannot reach cluster $KIND_CONTEXT"
    exit 1
  }

  # Clean any leftover state
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
  log "=== KWOK Verification Summary ==="
  for label in "${VARIANT_LABELS[@]}"; do
    local summary="$RESULTS_DIR/$label/summary.json"
    if [ -f "$summary" ]; then
      local fn mn ev
      fn=$(python3 -c "import json; print(json.load(open('$summary'))['final_node_count'])" 2>/dev/null) || fn="?"
      mn=$(python3 -c "import json; print(json.load(open('$summary'))['mid_node_count'])" 2>/dev/null) || mn="?"
      ev=$(python3 -c "import json; print(json.load(open('$summary'))['evictions'])" 2>/dev/null) || ev="?"
      log "  $label: final=$fn mid=$mn evictions=$ev"
    fi
  done

  log "Results in: $RESULTS_DIR"
  [ "$failed" -eq 0 ] || exit 1
}

main "$@"
