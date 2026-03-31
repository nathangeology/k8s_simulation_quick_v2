#!/usr/bin/env bash
# scale-sequence.sh — Replicate the simulator's scale-out/scale-in pattern
# Matches benchmark-control scenario timeline:
#   t=0:   deploy (1 replica each)
#   t=10s: scale up to 500
#   t=15m: scale down to 350
#   t=25m: scale down to 10
#   t=35m: end
set -euo pipefail

NAMESPACE="${NAMESPACE:-default}"

log() { echo "[$(date -u +%H:%M:%S)] $*"; }

log "t=0: Workloads deployed at 1 replica each"

log "Waiting 10s before scale-up..."
sleep 10

log "t=10s: Scaling to 500 replicas"
kubectl scale deployment workload-a workload-b --replicas=500 -n "$NAMESPACE"

log "Waiting 14m50s for scale-down phase 1..."
sleep 890

log "t=15m: Scaling workload-a down to 350 replicas"
kubectl scale deployment workload-a --replicas=350 -n "$NAMESPACE"
sleep 60
log "t=16m: Scaling workload-b down to 350 replicas"
kubectl scale deployment workload-b --replicas=350 -n "$NAMESPACE"

log "Waiting 9m for scale-down phase 2..."
sleep 540

log "t=25m: Scaling workload-a down to 10 replicas"
kubectl scale deployment workload-a --replicas=10 -n "$NAMESPACE"
sleep 60
log "t=26m: Scaling workload-b down to 10 replicas"
kubectl scale deployment workload-b --replicas=10 -n "$NAMESPACE"

log "Waiting 10m for consolidation to settle..."
sleep 600

log "t=35m: Scale sequence complete"
