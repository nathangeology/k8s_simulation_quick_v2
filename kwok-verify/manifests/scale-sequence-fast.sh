#!/usr/bin/env bash
# scale-sequence-fast.sh — Shortened scale sequence for pipeline validation (~2min total)
# Same phases as full sequence but with compressed timings.
set -euo pipefail

NAMESPACE="${NAMESPACE:-default}"
log() { echo "[$(date -u +%H:%M:%S)] $*"; }

log "t=0: Workloads deployed at 1 replica each"
sleep 5

log "t=5s: Scaling to 50 replicas"
kubectl scale deployment workload-a workload-b --replicas=50 -n "$NAMESPACE"
sleep 30

log "t=35s: Scaling down to 30 replicas"
kubectl scale deployment workload-a workload-b --replicas=30 -n "$NAMESPACE"
sleep 30

log "t=65s: Scaling down to 2 replicas"
kubectl scale deployment workload-a workload-b --replicas=2 -n "$NAMESPACE"
sleep 45

log "t=110s: Scale sequence complete"
