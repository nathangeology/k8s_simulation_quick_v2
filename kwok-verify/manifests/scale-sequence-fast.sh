#!/usr/bin/env bash
# scale-sequence-fast.sh — Shortened scale sequence for pipeline validation (~2min total)
# Same phases as full sequence but with compressed timings.
set -euo pipefail

NAMESPACE="${NAMESPACE:-default}"
log() { echo "[$(date -u +%H:%M:%S)] $*"; }

log "t=0: Workloads deployed at 1 replica each"
sleep 5

log "t=5s: Scaling to 500 replicas"
kubectl scale deployment workload-a workload-b --replicas=500 -n "$NAMESPACE"
sleep 30

log "t=35s: Scaling workload-a down to 350 replicas"
kubectl scale deployment workload-a --replicas=350 -n "$NAMESPACE"
sleep 15
log "t=50s: Scaling workload-b down to 350 replicas"
kubectl scale deployment workload-b --replicas=350 -n "$NAMESPACE"
sleep 15

log "t=65s: Scaling workload-a down to 10 replicas"
kubectl scale deployment workload-a --replicas=10 -n "$NAMESPACE"
sleep 15
log "t=80s: Scaling workload-b down to 10 replicas"
kubectl scale deployment workload-b --replicas=10 -n "$NAMESPACE"
sleep 30

log "t=110s: Scale sequence complete"
