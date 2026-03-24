#!/usr/bin/env bash
# run-kwok-full-verify.sh — Full 10-variant kwok verification with pod scheduling gate (R5)
# Each variant: deploy → scale to 500 → wait for Running → scale-down phases → collect
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/kwok-verify"
NAMESPACE="${NAMESPACE:-default}"
METRICS_INTERVAL="${METRICS_INTERVAL:-30}"

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

log() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*"; }
die() { log "FATAL: $*"; exit 1; }

# R5: Wait for all pods in deployments to be Running
wait_pods_running() {
  local target="$1"
  local timeout="${2:-300}"
  local start=$SECONDS
  log "R5: Waiting for $target pods to be Running (timeout ${timeout}s)..."
  while true; do
    local ready_a ready_b
    ready_a=$(kubectl get deployment workload-a -n "$NAMESPACE" -o jsonpath='{.status.readyReplicas}' 2>/dev/null) || ready_a=0
    ready_b=$(kubectl get deployment workload-b -n "$NAMESPACE" -o jsonpath='{.status.readyReplicas}' 2>/dev/null) || ready_b=0
    [ -z "$ready_a" ] && ready_a=0
    [ -z "$ready_b" ] && ready_b=0
    if [ "$ready_a" -ge "$target" ] && [ "$ready_b" -ge "$target" ]; then
      log "R5: All $target pods Running for both deployments (${ready_a}a/${ready_b}b)"
      return 0
    fi
    if [ $((SECONDS - start)) -ge "$timeout" ]; then
      log "R5: TIMEOUT after ${timeout}s — ready: ${ready_a}a/${ready_b}b of $target"
      return 1
    fi
    sleep 5
  done
}

collect_timeseries() {
  local out_dir="$1"
  local ts_file="$out_dir/timeseries.jsonl"
  > "$ts_file"
  while true; do
    local ts nodes pods pending
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    nodes=$(kubectl get nodes --no-headers 2>/dev/null | grep -v control-plane | wc -l | tr -d ' ')
    pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pending=$(kubectl get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    echo "{\"ts\":\"$ts\",\"nodes\":$nodes,\"pods\":$pods,\"pending\":$pending}" >> "$ts_file"
    sleep "$METRICS_INTERVAL"
  done
}

run_variant() {
  local variant="$1"
  local variant_dir="$RESULTS_DIR/$variant"
  local template="$TEMPLATES_DIR/${variant}.yaml"

  [ -f "$template" ] || die "Template not found: $template"
  mkdir -p "$variant_dir"

  log "=== Running variant: $variant ==="

  # 1. Apply NodePool
  log "Applying NodePool: $variant"
  kubectl apply -f "$template"
  sleep 10

  # 2. Deploy workloads at 1 replica
  log "Deploying workloads (1 replica each)"
  kubectl apply -f "$MANIFESTS_DIR/deployment-a.yaml" -n "$NAMESPACE"
  kubectl apply -f "$MANIFESTS_DIR/deployment-b.yaml" -n "$NAMESPACE"
  wait_pods_running 1 120

  # 3. Start timeseries collection
  collect_timeseries "$variant_dir" &
  local ts_pid=$!

  # 4. Scale sequence with R5 gates
  log "t=10s: Scaling to 500 replicas"
  sleep 10
  kubectl scale deployment workload-a workload-b --replicas=500 -n "$NAMESPACE"
  wait_pods_running 500 600  # R5: wait up to 10min for all 500 to be Running

  log "Waiting 14m50s for consolidation observation..."
  sleep 890

  log "t=15m: Scaling down to 350 replicas"
  kubectl scale deployment workload-a workload-b --replicas=350 -n "$NAMESPACE"

  log "Waiting 10m for scale-down phase 2..."
  sleep 600

  log "t=25m: Scaling down to 10 replicas"
  kubectl scale deployment workload-a workload-b --replicas=10 -n "$NAMESPACE"

  log "Waiting 10m for consolidation to settle..."
  sleep 600

  log "t=35m: Scale sequence complete"

  # 5. Stop timeseries
  kill "$ts_pid" 2>/dev/null || true
  wait "$ts_pid" 2>/dev/null || true

  # 6. Collect Karpenter consolidation logs
  log "Collecting Karpenter logs"
  kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter --since=40m \
    | grep -E '(disrupting|consolidat|decision.ratio)' \
    > "$variant_dir/karpenter-consolidation.log" 2>/dev/null || true

  # 7. Count evictions
  local evictions
  evictions=$(grep -c 'disrupting node' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || evictions=0

  # 8. Final snapshot
  local nodes pods
  nodes=$(kubectl get nodes --no-headers 2>/dev/null | grep -v control-plane | wc -l | tr -d ' ')
  pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  cat > "$variant_dir/summary.json" <<EOF
{
  "variant": "$variant",
  "final_node_count": $nodes,
  "final_pod_count": $pods,
  "pods_evicted": $evictions,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "mode": "full"
}
EOF

  # 9. Cleanup
  log "Cleaning up variant: $variant"
  kubectl delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found
  kubectl delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 15

  log "=== Variant $variant complete: $evictions evictions, $nodes final nodes ==="
}

main() {
  log "Starting full kwok verification (10 variants, ~35min each)"
  mkdir -p "$RESULTS_DIR"

  # Verify cluster
  kubectl cluster-info >/dev/null 2>&1 || die "Cannot reach cluster"
  kubectl get pods -n kube-system -l app.kubernetes.io/name=karpenter --no-headers | grep -q Running || die "Karpenter not running"

  local failed=0
  for variant in "${VARIANTS[@]}"; do
    if ! run_variant "$variant"; then
      log "ERROR: Variant $variant failed"
      failed=$((failed + 1))
    fi
  done

  log "Verification complete. $((${#VARIANTS[@]} - failed))/${#VARIANTS[@]} variants succeeded."
  log "Results in: $RESULTS_DIR"
  [ "$failed" -eq 0 ] || exit 1
}

main "$@"
