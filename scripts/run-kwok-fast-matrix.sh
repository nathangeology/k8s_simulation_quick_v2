#!/usr/bin/env bash
# run-kwok-fast-matrix.sh — Run all 10 variants with fast timings against existing cluster
# Assumes kind cluster + Karpenter already running (from prior setup)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/kwok-verify"
NAMESPACE="${NAMESPACE:-default}"
METRICS_INTERVAL="${METRICS_INTERVAL:-10}"

VARIANTS=(
  when-empty
  when-underutilized
  cost-justified-0.25
  cost-justified-0.50
  cost-justified-0.75
  cost-justified-1.00
  cost-justified-1.50
  cost-justified-2.00
  cost-justified-3.00
  cost-justified-5.00
)

# Allow running a single variant via --variant flag
if [[ "${1:-}" == "--variant" && -n "${2:-}" ]]; then
  VARIANTS=("$2")
fi

log() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*"; }

collect_timeseries() {
  local out_file="$1"
  > "$out_file"
  while true; do
    local ts nodes pods pending
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    nodes=$(kubectl get nodes --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pending=$(kubectl get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    echo "{\"ts\":\"$ts\",\"nodes\":$nodes,\"pods\":$pods,\"pending\":$pending}" >> "$out_file"
    sleep "$METRICS_INTERVAL"
  done
}

run_variant() {
  local variant="$1"
  local vdir="$RESULTS_DIR/$variant"
  local template="$TEMPLATES_DIR/${variant}.yaml"

  [ -f "$template" ] || { log "SKIP: template not found: $template"; return 1; }
  mkdir -p "$vdir"

  log "=== START: $variant ==="

  # Apply NodePool
  kubectl apply -f "$template" 2>&1
  sleep 5

  # Deploy workloads
  kubectl apply -f "$MANIFESTS_DIR/deployment-a.yaml" -n "$NAMESPACE" 2>&1
  kubectl apply -f "$MANIFESTS_DIR/deployment-b.yaml" -n "$NAMESPACE" 2>&1

  # Timeseries collector
  collect_timeseries "$vdir/timeseries.jsonl" &
  local ts_pid=$!

  # Fast scale sequence
  NAMESPACE="$NAMESPACE" "$MANIFESTS_DIR/scale-sequence-fast.sh"

  # Stop collector
  kill "$ts_pid" 2>/dev/null; wait "$ts_pid" 2>/dev/null || true

  # Karpenter logs
  kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter --since=5m \
    | grep -E '(disrupting|consolidat|decision.ratio)' \
    > "$vdir/karpenter-consolidation.log" 2>/dev/null || true

  # Count evictions
  local evictions
  evictions=$(grep -c 'disrupting node' "$vdir/karpenter-consolidation.log" 2>/dev/null) || evictions=0

  # Final snapshot
  local nodes pods
  nodes=$(kubectl get nodes --no-headers 2>/dev/null | wc -l | tr -d ' ')
  pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  cat > "$vdir/summary.json" <<EOF
{
  "variant": "$variant",
  "final_node_count": $nodes,
  "final_pod_count": $pods,
  "pods_evicted": $evictions,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "mode": "fast"
}
EOF

  # Cleanup
  kubectl delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found 2>&1
  kubectl delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 10

  log "=== DONE: $variant — evictions=$evictions nodes=$nodes ==="
}

main() {
  log "Fast matrix verification — ${#VARIANTS[@]} variants"
  mkdir -p "$RESULTS_DIR"

  kubectl cluster-info --context kind-kubesim-val >/dev/null 2>&1 || { log "FATAL: cluster not reachable"; exit 1; }

  local failed=0
  for variant in "${VARIANTS[@]}"; do
    if ! run_variant "$variant"; then
      log "ERROR: $variant failed"
      failed=$((failed + 1))
    fi
  done

  log "Complete: $((${#VARIANTS[@]} - failed))/${#VARIANTS[@]} succeeded"
  log "Results: $RESULTS_DIR"
  [ "$failed" -eq 0 ] || exit 1
}

main "$@"
