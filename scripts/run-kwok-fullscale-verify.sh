#!/usr/bin/env bash
# run-kwok-fullscale-verify.sh — Full-scale 4-variant KWOK verification (k8s-mlci)
# Variants: when-empty, when-underutilized, cost-justified-1.00, cost-justified-5.00
# Parameters: 500 replicas, 35min sequence, consolidateAfter=30s, 60s metrics
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/kwok-verify-fullscale"
NAMESPACE="${NAMESPACE:-default}"
METRICS_INTERVAL="${METRICS_INTERVAL:-60}"

VARIANTS=(
  when-empty
  when-underutilized
  cost-justified-1.00
  cost-justified-5.00
)

log() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*"; }
die() { log "FATAL: $*"; exit 1; }

wait_pods_running() {
  local target="$1" timeout="${2:-600}" start=$SECONDS
  log "Waiting for $target pods Running (timeout ${timeout}s)..."
  while true; do
    local ra rb
    ra=$(kubectl get deployment workload-a -n "$NAMESPACE" -o jsonpath='{.status.readyReplicas}' 2>/dev/null) || ra=0
    rb=$(kubectl get deployment workload-b -n "$NAMESPACE" -o jsonpath='{.status.readyReplicas}' 2>/dev/null) || rb=0
    [ -z "$ra" ] && ra=0; [ -z "$rb" ] && rb=0
    if [ "$ra" -ge "$target" ] && [ "$rb" -ge "$target" ]; then
      log "All $target pods Running (${ra}a/${rb}b)"
      return 0
    fi
    if [ $((SECONDS - start)) -ge "$timeout" ]; then
      log "TIMEOUT: ${ra}a/${rb}b of $target after ${timeout}s"
      return 1
    fi
    sleep 5
  done
}

collect_timeseries() {
  local ts_file="$1"
  > "$ts_file"
  while true; do
    local ts nodes pods pending
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    nodes=$(kubectl get nodes --no-headers 2>/dev/null | grep -cv control-plane || echo 0)
    pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pending=$(kubectl get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    echo "{\"ts\":\"$ts\",\"nodes\":$nodes,\"pods\":$pods,\"pending\":$pending}" >> "$ts_file"
    sleep "$METRICS_INTERVAL"
  done
}

# Stream karpenter logs from start into a file (background)
capture_karpenter_logs() {
  local out="$1"
  kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter -f --since=0s > "$out" 2>/dev/null &
  echo $!
}

run_variant() {
  local variant="$1"
  local vdir="$RESULTS_DIR/$variant"
  local template="$TEMPLATES_DIR/${variant}.yaml"
  [ -f "$template" ] || die "Template not found: $template"
  mkdir -p "$vdir"

  log "=== START: $variant ==="

  kubectl apply -f "$template"
  sleep 10

  kubectl apply -f "$MANIFESTS_DIR/deployment-a.yaml" -n "$NAMESPACE"
  kubectl apply -f "$MANIFESTS_DIR/deployment-b.yaml" -n "$NAMESPACE"
  wait_pods_running 1 120

  # Start background collectors
  collect_timeseries "$vdir/timeseries.jsonl" &
  local ts_pid=$!
  local log_pid
  log_pid=$(capture_karpenter_logs "$vdir/karpenter-full.log")

  # 35-min scale sequence
  sleep 10
  log "t=10s: Scaling to 500 replicas"
  kubectl scale deployment workload-a workload-b --replicas=500 -n "$NAMESPACE"
  wait_pods_running 500 600

  log "Waiting 14m50s..."
  sleep 890

  log "t=15m: Scaling workload-a down to 350"
  kubectl scale deployment workload-a --replicas=350 -n "$NAMESPACE"
  sleep 60
  log "t=16m: Scaling workload-b down to 350"
  kubectl scale deployment workload-b --replicas=350 -n "$NAMESPACE"

  log "Waiting 9m..."
  sleep 540

  log "t=25m: Scaling workload-a down to 10"
  kubectl scale deployment workload-a --replicas=10 -n "$NAMESPACE"
  sleep 60
  log "t=26m: Scaling workload-b down to 10"
  kubectl scale deployment workload-b --replicas=10 -n "$NAMESPACE"

  log "Waiting 10m for consolidation..."
  sleep 600

  log "t=35m: Sequence complete"

  # Stop collectors
  kill "$ts_pid" 2>/dev/null; wait "$ts_pid" 2>/dev/null || true
  kill "$log_pid" 2>/dev/null; wait "$log_pid" 2>/dev/null || true

  # Extract consolidation-relevant lines
  grep -E '(disrupting|consolidat|decision\.ratio|CostJustified)' \
    "$vdir/karpenter-full.log" > "$vdir/karpenter-consolidation.log" 2>/dev/null || true

  # Metrics
  local evictions cj_path dr_entries nodes pods
  evictions=$(grep -c 'disrupting node' "$vdir/karpenter-consolidation.log" 2>/dev/null) || evictions=0
  cj_path=$(grep -c 'CostJustified/' "$vdir/karpenter-consolidation.log" 2>/dev/null) || cj_path=0
  dr_entries=$(grep -c 'decision.ratio' "$vdir/karpenter-consolidation.log" 2>/dev/null) || dr_entries=0
  nodes=$(kubectl get nodes --no-headers 2>/dev/null | grep -cv control-plane || echo 0)
  pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  cat > "$vdir/summary.json" <<EOF
{
  "variant": "$variant",
  "final_node_count": $nodes,
  "final_pod_count": $pods,
  "pods_evicted": $evictions,
  "cost_justified_path": $cj_path,
  "decision_ratio_entries": $dr_entries,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "mode": "fullscale",
  "replicas": 500,
  "sequence_minutes": 35,
  "metrics_interval_s": $METRICS_INTERVAL
}
EOF

  # Cleanup
  kubectl delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found
  kubectl delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 15

  log "=== DONE: $variant — evictions=$evictions nodes=$nodes ==="
}

main() {
  log "Full-scale 4-variant KWOK verification (500 replicas, 35min each, ~2h20m total)"
  mkdir -p "$RESULTS_DIR"

  kubectl cluster-info >/dev/null 2>&1 || die "Cannot reach cluster"
  kubectl get pods -n kube-system -l app.kubernetes.io/name=karpenter --no-headers | grep -q Running \
    || die "Karpenter not running"

  local commit
  commit=$(kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter --tail=1 2>/dev/null \
    | grep -o '"commit":"[^"]*"' | head -1) || true
  log "Karpenter build: $commit"

  local failed=0
  for variant in "${VARIANTS[@]}"; do
    if ! run_variant "$variant"; then
      log "ERROR: $variant failed"
      failed=$((failed + 1))
    fi
  done

  log "Complete: $((${#VARIANTS[@]} - failed))/${#VARIANTS[@]} succeeded"
  log "Results: $RESULTS_DIR"

  # Generate comparison report
  "$SCRIPT_DIR/generate-fullscale-report.sh"

  [ "$failed" -eq 0 ] || exit 1
}

main "$@"
