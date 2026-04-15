#!/usr/bin/env bash
# run-kwok-3variant-balanced.sh — 3-variant KWOK verification:
#   when-empty / balanced-k2 / when-underutilized
# Full heterogeneous fleet, 500 replicas, timed scale sequence.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/kwok-verify/templates-hetero"
MANIFESTS_DIR="$REPO_ROOT/kwok-verify/manifests"
RESULTS_DIR="$REPO_ROOT/results/kwok-balanced-threshold"
NAMESPACE="${NAMESPACE:-default}"
METRICS_INTERVAL="${METRICS_INTERVAL:-60}"
KIND_CONTEXT="${KIND_CONTEXT:-kind-kubesim}"
REPLICAS=500
SCALE_MID=350
SCALE_LOW=10
# Timing: scale-up at t=0, 500→350 at 15min, 350→10 at 25min, end at 35min
T_SCALEDOWN1=$((15 * 60))
T_SCALEDOWN2=$((25 * 60))
T_END=$((35 * 60))

VARIANTS=(when-empty balanced-k2 when-underutilized)

# Track background PIDs for cleanup
BG_PIDS=()

log() { echo "[$(date -u +%H:%M:%S)] $*"; }

kill_bg() {
  for pid in "${BG_PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  # Wait briefly for children to exit
  for pid in "${BG_PIDS[@]}"; do
    wait "$pid" 2>/dev/null || true
  done
  BG_PIDS=()
}

cleanup_variant() {
  log "Cleaning up..."
  kill_bg
  kubectl --context "$KIND_CONTEXT" delete deployment workload-a workload-b -n "$NAMESPACE" --ignore-not-found 2>/dev/null || true
  sleep 5
  kubectl --context "$KIND_CONTEXT" delete nodes -l karpenter.sh/nodepool=default --ignore-not-found 2>/dev/null || true
  kubectl --context "$KIND_CONTEXT" delete nodepool default --ignore-not-found 2>/dev/null || true
  sleep 15
  local remaining
  remaining=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  if [ "$remaining" -gt 0 ]; then
    log "WARNING: $remaining nodes still present, waiting 30s..."
    sleep 30
  fi
}

collect_timeseries() {
  local out_file="$1"
  > "$out_file"
  while true; do
    local ts nodes pods pending
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
    pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers --field-selector=status.phase=Running 2>/dev/null | wc -l | tr -d ' ')
    pending=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    echo "{\"ts\":\"$ts\",\"nodes\":$nodes,\"pods\":$pods,\"pending\":$pending}" >> "$out_file"
    sleep "$METRICS_INTERVAL"
  done
}

wait_pods_scheduled() {
  local max_wait=300
  local waited=0
  while [ "$waited" -lt "$max_wait" ]; do
    local pending
    pending=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --field-selector=status.phase=Pending --no-headers 2>/dev/null | wc -l | tr -d ' ')
    if [ "$pending" -eq 0 ]; then
      log "All pods scheduled (waited ${waited}s)"
      return 0
    fi
    sleep 5
    waited=$((waited + 5))
  done
  log "WARNING: ${pending:-?} pods still pending after ${max_wait}s"
}

# Robust sleep that won't get stuck
safe_sleep() {
  local duration=$1
  local slept=0
  local chunk=30
  while [ "$slept" -lt "$duration" ]; do
    local remaining=$((duration - slept))
    if [ "$remaining" -lt "$chunk" ]; then
      chunk=$remaining
    fi
    sleep "$chunk"
    slept=$((slept + chunk))
  done
}

run_variant() {
  local variant="$1"
  local template="$TEMPLATES_DIR/${variant}.yaml"
  local variant_dir="$RESULTS_DIR/$variant"

  [ -f "$template" ] || { log "ERROR: Template not found: $template"; return 1; }
  mkdir -p "$variant_dir"

  log "=== Running variant: $variant ==="

  # Start capturing Karpenter logs from the beginning
  kubectl --context "$KIND_CONTEXT" logs -n kube-system -l app.kubernetes.io/name=karpenter -f \
    > "$variant_dir/karpenter-full.log" 2>/dev/null &
  BG_PIDS+=($!)

  # Apply NodePool
  log "Applying NodePool ($variant)"
  kubectl --context "$KIND_CONTEXT" apply -f "$template"
  sleep 5

  # Deploy workloads at 1 replica
  log "Deploying workloads"
  kubectl --context "$KIND_CONTEXT" apply -f "$MANIFESTS_DIR/deployment-a.yaml" -n "$NAMESPACE"
  kubectl --context "$KIND_CONTEXT" apply -f "$MANIFESTS_DIR/deployment-b.yaml" -n "$NAMESPACE"
  sleep 5

  # Start timeseries collection
  collect_timeseries "$variant_dir/timeseries.jsonl" &
  BG_PIDS+=($!)

  # Record start time
  local start_epoch
  start_epoch=$(date +%s)

  # Scale up to 500
  log "Scaling to $REPLICAS replicas"
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=$REPLICAS -n "$NAMESPACE"
  wait_pods_scheduled

  # Wait until T_SCALEDOWN1 (15min mark)
  local now_epoch elapsed remaining_wait
  now_epoch=$(date +%s)
  elapsed=$((now_epoch - start_epoch))
  remaining_wait=$((T_SCALEDOWN1 - elapsed))
  if [ "$remaining_wait" -gt 0 ]; then
    log "Waiting ${remaining_wait}s until 15min mark for first scale-down..."
    safe_sleep "$remaining_wait"
  fi

  # Window 1: 500->350
  log "=== Window 1: Scaling ${REPLICAS}->${SCALE_MID} ==="
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=$SCALE_MID -n "$NAMESPACE"

  # Capture window 1 snapshot after consolidation settles
  local w1_nodes_before
  w1_nodes_before=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')

  # Wait until T_SCALEDOWN2 (25min mark)
  now_epoch=$(date +%s)
  elapsed=$((now_epoch - start_epoch))
  remaining_wait=$((T_SCALEDOWN2 - elapsed))
  if [ "$remaining_wait" -gt 0 ]; then
    log "Waiting ${remaining_wait}s until 25min mark for second scale-down..."
    safe_sleep "$remaining_wait"
  fi

  local w1_nodes_after
  w1_nodes_after=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  log "Window 1 result: ${w1_nodes_before} -> ${w1_nodes_after} nodes"

  # Window 2: 350->10
  log "=== Window 2: Scaling ${SCALE_MID}->${SCALE_LOW} ==="
  kubectl --context "$KIND_CONTEXT" scale deployment workload-a workload-b --replicas=$SCALE_LOW -n "$NAMESPACE"

  local w2_nodes_before
  w2_nodes_before=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')

  # Wait until T_END (35min mark)
  now_epoch=$(date +%s)
  elapsed=$((now_epoch - start_epoch))
  remaining_wait=$((T_END - elapsed))
  if [ "$remaining_wait" -gt 0 ]; then
    log "Waiting ${remaining_wait}s until 35min mark (end)..."
    safe_sleep "$remaining_wait"
  fi

  # Stop background processes
  kill_bg

  # Final snapshot
  local final_nodes final_pods
  final_nodes=$(kubectl --context "$KIND_CONTEXT" get nodes --no-headers -l karpenter.sh/nodepool=default 2>/dev/null | wc -l | tr -d ' ')
  final_pods=$(kubectl --context "$KIND_CONTEXT" get pods -n "$NAMESPACE" --no-headers 2>/dev/null | wc -l | tr -d ' ')

  # Count disruptions from logs
  local node_disruptions pod_evictions
  node_disruptions=$(grep -c 'disrupting node\|disrupting nodeclaim' "$variant_dir/karpenter-full.log" 2>/dev/null) || node_disruptions=0
  pod_evictions=$(grep -c 'evicting pod' "$variant_dir/karpenter-full.log" 2>/dev/null) || pod_evictions=0

  # Count by consolidation path
  local empty_path underutil_path balanced_path
  empty_path=$(grep -c -i 'consolidation.*empty\|"Empty"\|Empty/' "$variant_dir/karpenter-full.log" 2>/dev/null) || empty_path=0
  underutil_path=$(grep -c -i 'consolidation.*underutilized\|"Underutilized"\|Underutilized/' "$variant_dir/karpenter-full.log" 2>/dev/null) || underutil_path=0
  balanced_path=$(grep -c -i 'consolidation.*balanced\|"Balanced"\|Balanced/\|CostJustified\|decision.ratio' "$variant_dir/karpenter-full.log" 2>/dev/null) || balanced_path=0

  # Extract consolidation log lines
  grep -iE '(disrupting|consolidat|decision.ratio|CostJustified|Balanced|Empty|Underutilized|evicting)' \
    "$variant_dir/karpenter-full.log" > "$variant_dir/karpenter-consolidation.log" 2>/dev/null || true

  # Node count timeseries from jsonl
  python3 -c "
import json
with open('$variant_dir/timeseries.jsonl') as f:
    entries = [json.loads(l) for l in f if l.strip()]
print(json.dumps([{'ts': e['ts'], 'nodes': e['nodes']} for e in entries], indent=2))
" > "$variant_dir/node-timeseries.json" 2>/dev/null || true

  cat > "$variant_dir/summary.json" <<EOF
{
  "variant": "$variant",
  "final_node_count": $final_nodes,
  "final_pod_count": $final_pods,
  "window1_nodes_before": $w1_nodes_before,
  "window1_nodes_after": $w1_nodes_after,
  "window2_nodes_before": $w2_nodes_before,
  "node_disruptions": $node_disruptions,
  "pod_evictions": $pod_evictions,
  "empty_path_entries": $empty_path,
  "underutil_path_entries": $underutil_path,
  "balanced_path_entries": $balanced_path,
  "collected_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

  log "=== $variant: final=$final_nodes nodes, disrupted=$node_disruptions, evictions=$pod_evictions ==="

  cleanup_variant
}

generate_report() {
  log "Generating comparison report..."
  python3 - "$RESULTS_DIR" <<'PYEOF'
import json, sys, os

results_dir = sys.argv[1]
variants = ["when-empty", "balanced-k2", "when-underutilized"]
sim_predictions = {
    "when-empty":        {"disruptions": 0,   "final_nodes": 3.6},
    "balanced-k2":       {"disruptions": 140,  "final_nodes": 1.0},
    "when-underutilized":{"disruptions": 516,  "final_nodes": 1.0},
}

data = {}
for v in variants:
    path = os.path.join(results_dir, v, "summary.json")
    if os.path.exists(path):
        with open(path) as f:
            data[v] = json.load(f)

lines = ["# KWOK 3-Variant Balanced Threshold Verification", ""]
lines.append("## Parameters")
lines.append("- 500 replicas × 2 deployments, scale: 500→350 at 15min, 350→10 at 25min")
lines.append("- consolidateAfter: 30s, full heterogeneous KWOK fleet (amd64+linux)")
lines.append("- 60s metrics intervals, 35min total per variant")
lines.append("")

lines.append("## Results")
lines.append("")
lines.append("| Variant | Disrupted Nodes | Pod Evictions | Final Nodes | Empty Path | Underutil Path | Balanced Path |")
lines.append("|---------|----------------|---------------|-------------|------------|----------------|---------------|")
for v in variants:
    d = data.get(v, {})
    lines.append(f"| {v} | {d.get('node_disruptions','?')} | {d.get('pod_evictions','?')} | {d.get('final_node_count','?')} | {d.get('empty_path_entries','?')} | {d.get('underutil_path_entries','?')} | {d.get('balanced_path_entries','?')} |")

lines.append("")
lines.append("## Window Analysis")
lines.append("")
lines.append("| Variant | W1 Before (500→350) | W1 After | W2 Before (350→10) | Final |")
lines.append("|---------|---------------------|----------|--------------------|-------|")
for v in variants:
    d = data.get(v, {})
    lines.append(f"| {v} | {d.get('window1_nodes_before','?')} | {d.get('window1_nodes_after','?')} | {d.get('window2_nodes_before','?')} | {d.get('final_node_count','?')} |")

lines.append("")
lines.append("## Sim vs KWOK Comparison")
lines.append("")
lines.append("| Variant | Sim Disruptions | KWOK Disruptions | Sim Final Nodes | KWOK Final Nodes |")
lines.append("|---------|----------------|------------------|-----------------|------------------|")
for v in variants:
    d = data.get(v, {})
    sp = sim_predictions[v]
    lines.append(f"| {v} | {sp['disruptions']} | {d.get('node_disruptions','?')} | {sp['final_nodes']} | {d.get('final_node_count','?')} |")

lines.append("")

report = "\n".join(lines)
report_path = os.path.join(results_dir, "report.md")
with open(report_path, "w") as f:
    f.write(report)
print(f"Report written to {report_path}")

# Also write combined JSON
combined_path = os.path.join(results_dir, "all-results.json")
with open(combined_path, "w") as f:
    json.dump(data, f, indent=2)
print(f"Combined results written to {combined_path}")
PYEOF
}

main() {
  log "Starting 3-variant KWOK balanced threshold verification"
  log "Cluster: $KIND_CONTEXT | Replicas: $REPLICAS | Metrics: ${METRICS_INTERVAL}s"
  mkdir -p "$RESULTS_DIR"

  kubectl --context "$KIND_CONTEXT" cluster-info > /dev/null 2>&1 || {
    log "ERROR: Cannot reach cluster $KIND_CONTEXT"
    exit 1
  }

  cleanup_variant

  local failed=0
  for variant in "${VARIANTS[@]}"; do
    if ! run_variant "$variant"; then
      log "ERROR: Variant $variant failed"
      failed=$((failed + 1))
    fi
  done

  generate_report

  log ""
  log "=== Final Summary ==="
  for variant in "${VARIANTS[@]}"; do
    local summary="$RESULTS_DIR/$variant/summary.json"
    if [ -f "$summary" ]; then
      log "  $variant: $(python3 -c "import json; d=json.load(open('$summary')); print(f'final={d[\"final_node_count\"]} disrupted={d[\"node_disruptions\"]} evictions={d[\"pod_evictions\"]}')")"
    fi
  done

  log "Results: $RESULTS_DIR"
  [ "$failed" -eq 0 ] || exit 1
}

main "$@"
