#!/bin/bash
set -euo pipefail

# Collect cluster metrics every INTERVAL seconds, output JSON timeseries
INTERVAL=${INTERVAL:-30}
OUTPUT=${1:-/dev/stdout}

log() { echo "[$(date +%H:%M:%S)] $1" >&2; }

start_time=$(date +%s)
echo "[" > "$OUTPUT"
first=true

cleanup() {
    # Close JSON array
    echo "]" >> "$OUTPUT"
    log "Metrics collection stopped. Output: $OUTPUT"
    exit 0
}
trap cleanup SIGINT SIGTERM

log "Collecting metrics every ${INTERVAL}s → $OUTPUT"

while true; do
    now=$(date +%s)
    elapsed=$((now - start_time))
    time_ns=$((elapsed * 1000000000))

    # Count nodes (exclude control-plane and real worker)
    kwok_nodes=$(kubectl get nodes -l 'karpenter.sh/nodepool' --no-headers 2>/dev/null || true)
    node_count=0
    if [ -n "$kwok_nodes" ]; then
        node_count=$(echo "$kwok_nodes" | grep -c "Ready" || true)
    fi

    # Compute cost from node allocatable CPU (approximate: $0.048/vCPU/hr on-demand)
    cost_per_hour=0
    if [ "$node_count" -gt 0 ]; then
        cost_per_hour=$(kubectl get nodes -l 'karpenter.sh/nodepool' -o json 2>/dev/null | python3 -c "
import json, sys
data = json.load(sys.stdin)
cost = 0
for n in data.get('items', []):
    # Get price from karpenter label if available, else estimate from CPU
    labels = n.get('metadata', {}).get('labels', {})
    price_str = labels.get('karpenter.kwok.sh/instance-price', '')
    if price_str:
        cost += float(price_str)
    else:
        alloc = n.get('status', {}).get('allocatable', {})
        cpu_str = alloc.get('cpu', '0')
        vcpu = int(cpu_str[:-1])/1000 if cpu_str.endswith('m') else int(cpu_str)
        cost += vcpu * 0.048  # ~m5 pricing
print(f'{cost:.4f}')
" 2>/dev/null || echo "0")
    fi

    # Count pods
    all_pods=$(kubectl get pods --all-namespaces --field-selector='status.phase!=Succeeded,status.phase!=Failed' --no-headers 2>/dev/null || true)
    pod_count=$(echo "$all_pods" | grep -v "^$" | wc -l | tr -d ' ')
    pending_count=0
    if [ -n "$all_pods" ]; then
        pending_count=$(echo "$all_pods" | grep -c "Pending" || true)
    fi

    # Compute total allocated vCPU and memory on kwok nodes
    total_vcpu=0
    total_mem_gib=0
    if [ "$node_count" -gt 0 ]; then
        alloc=$(kubectl get nodes -l 'karpenter.sh/nodepool' -o json 2>/dev/null | python3 -c "
import json, sys
data = json.load(sys.stdin)
vcpu = 0; mem = 0
for n in data.get('items', []):
    alloc = n.get('status', {}).get('allocatable', {})
    cpu_str = alloc.get('cpu', '0')
    mem_str = alloc.get('memory', '0')
    # Parse CPU
    if cpu_str.endswith('m'):
        vcpu += int(cpu_str[:-1]) / 1000
    else:
        vcpu += int(cpu_str)
    # Parse memory
    if mem_str.endswith('Ki'):
        mem += int(mem_str[:-2]) / 1024 / 1024
    elif mem_str.endswith('Mi'):
        mem += int(mem_str[:-2]) / 1024
    elif mem_str.endswith('Gi'):
        mem += int(mem_str[:-2])
print(f'{vcpu},{mem:.2f}')
" 2>/dev/null || echo "0,0")
        total_vcpu=$(echo "$alloc" | cut -d, -f1)
        total_mem_gib=$(echo "$alloc" | cut -d, -f2)
    fi

    # Emit JSON record
    if [ "$first" = true ]; then
        first=false
    else
        echo "," >> "$OUTPUT"
    fi

    cat >> "$OUTPUT" <<JSONEOF
  {
    "time": $time_ns,
    "elapsed_s": $elapsed,
    "node_count": $node_count,
    "pod_count": $pod_count,
    "pending_count": $pending_count,
    "total_cost_per_hour": $cost_per_hour,
    "total_vcpu_allocated": $total_vcpu,
    "total_memory_allocated_gib": $total_mem_gib
  }
JSONEOF

    log "t=${elapsed}s nodes=$node_count pods=$pod_count pending=$pending_count cost=\$${cost_per_hour}/hr"

    sleep "$INTERVAL"
done
