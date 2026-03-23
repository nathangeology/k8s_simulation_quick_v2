#!/usr/bin/env bash
# run-kwok-consolidate-verify.sh — Orchestrate kwok verification of ConsolidateWhen variants
# Runs all 10 variants sequentially against a kind+kwok cluster with Karpenter from PR #2893
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/kwok-verify"
WORK_DIR="$REPO_ROOT/validation/.work"
COLLECT_METRICS="$REPO_ROOT/validation/collect-metrics.sh"

KIND_CLUSTER_NAME="${KIND_CLUSTER_NAME:-kubesim-val}"
KARPENTER_REF="${KARPENTER_REF:-pull/2893/head}"
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

# --- Cluster Setup ---

setup_cluster() {
  log "Setting up kind cluster: $KIND_CLUSTER_NAME"
  if kind get clusters 2>/dev/null | grep -q "^${KIND_CLUSTER_NAME}$"; then
    log "Cluster already exists, reusing"
  else
    kind create cluster --name "$KIND_CLUSTER_NAME" --wait 120s
  fi
  kubectl cluster-info --context "kind-${KIND_CLUSTER_NAME}" || die "Cannot reach cluster"
}

build_karpenter() {
  log "Building Karpenter from ref: $KARPENTER_REF"
  mkdir -p "$WORK_DIR"

  if [ ! -d "$WORK_DIR/karpenter" ]; then
    git clone https://github.com/kubernetes-sigs/karpenter.git "$WORK_DIR/karpenter"
  fi

  cd "$WORK_DIR/karpenter"
  git fetch origin

  if [[ "$KARPENTER_REF" == pull/* ]]; then
    local pr_num
    pr_num=$(echo "$KARPENTER_REF" | cut -d/ -f2)
    git fetch origin "pull/${pr_num}/head:pr-${pr_num}"
    git checkout "pr-${pr_num}"
  else
    git checkout "$KARPENTER_REF"
  fi

  log "Building with KWOK provider and loading into kind"
  export KIND_CLUSTER_NAME
  make build-with-kind || die "Karpenter build failed"

  log "Applying CRDs"
  kubectl apply -f kwok/charts/crds/

  log "Installing Karpenter via Helm"
  helm upgrade --install karpenter kwok/charts --namespace kube-system --skip-crds \
    --set controller.image.repository=kind.local/karpenter \
    --set controller.image.tag=latest \
    --set serviceMonitor.enabled=true \
    --wait --timeout 300s

  cd "$REPO_ROOT"
}

# --- Per-Variant Execution ---

run_variant() {
  local variant="$1"
  local variant_dir="$RESULTS_DIR/$variant"
  local template="$TEMPLATES_DIR/${variant}.yaml"

  [ -f "$template" ] || die "Template not found: $template"
  mkdir -p "$variant_dir"

  log "=== Running variant: $variant ==="

  # 1. Apply NodePool
  log "Applying NodePool template: $variant"
  kubectl apply -f "$template"
  sleep 10

  # 2. Deploy workloads
  log "Deploying workloads"
  kubectl apply -f "$MANIFESTS_DIR/deployment-a.yaml" -n "$NAMESPACE"
  kubectl apply -f "$MANIFESTS_DIR/deployment-b.yaml" -n "$NAMESPACE"

  # 3. Start metrics collection in background
  local metrics_pid=""
  if [ -x "$COLLECT_METRICS" ]; then
    RESULTS_DIR="$variant_dir" "$COLLECT_METRICS" &
    metrics_pid=$!
    log "Metrics collection started (pid: $metrics_pid)"
  fi

  # 4. Start timeseries collection in background
  collect_timeseries "$variant_dir" &
  local ts_pid=$!

  # 5. Run scale sequence
  log "Running scale sequence"
  NAMESPACE="$NAMESPACE" "$MANIFESTS_DIR/scale-sequence.sh"

  # 6. Stop background collectors
  [ -n "$metrics_pid" ] && kill "$metrics_pid" 2>/dev/null || true
  kill "$ts_pid" 2>/dev/null || true
  wait "$ts_pid" 2>/dev/null || true

  # 7. Collect Karpenter logs
  log "Collecting Karpenter consolidation logs"
  kubectl logs -n kube-system -l app.kubernetes.io/name=karpenter --since=40m \
    | grep -E '(disrupting|consolidat|decision.ratio)' \
    > "$variant_dir/karpenter-consolidation.log" 2>/dev/null || true

  # 8. Count evictions
  local evictions
  evictions=$(grep -c 'disrupting node' "$variant_dir/karpenter-consolidation.log" 2>/dev/null) || evictions=0

  # 9. Collect final snapshot
  collect_final_snapshot "$variant_dir" "$evictions"

  # 10. Cleanup workloads and nodes
  log "Cleaning up variant: $variant"
  kubectl delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found
  kubectl delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 10

  log "=== Variant $variant complete: $evictions evictions ==="
}

collect_timeseries() {
  local out_dir="$1"
  local ts_file="$out_dir/timeseries.jsonl"
  > "$ts_file"

  while true; do
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    local nodes pods pending
    nodes=$(kubectl get nodes --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')
    pending=$(kubectl get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    echo "{\"ts\":\"$ts\",\"nodes\":$nodes,\"pods\":$pods,\"pending\":$pending}" >> "$ts_file"
    sleep "$METRICS_INTERVAL"
  done
}

collect_final_snapshot() {
  local out_dir="$1"
  local evictions="$2"

  local nodes pods
  nodes=$(kubectl get nodes --no-headers 2>/dev/null | wc -l | tr -d ' ')
  pods=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  cat > "$out_dir/summary.json" <<EOF
{
  "final_node_count": $nodes,
  "final_pod_count": $pods,
  "pods_evicted": $evictions,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
}

# --- Main ---

main() {
  log "Starting kwok ConsolidateWhen verification"
  log "Variants: ${VARIANTS[*]}"
  mkdir -p "$RESULTS_DIR"

  # Setup
  setup_cluster
  build_karpenter

  # Run all variants
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
