#!/bin/bash
set -euo pipefail

# Run a kubesim scenario on a real KIND+Karpenter cluster
# Usage: run-scenario.sh <scenario> [--smoke]
#   scenario: benchmark-control | smoke-test
#   --smoke: run a quick 2-minute smoke test

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SCENARIO=${1:-smoke-test}
RESULTS_DIR="${SCRIPT_DIR}/results/${SCENARIO}"
mkdir -p "$RESULTS_DIR"

log() { echo "[$(date +%H:%M:%S)] $1"; }

# Start metrics collection in background
log "Starting metrics collection..."
bash "$SCRIPT_DIR/collect-metrics.sh" "$RESULTS_DIR/timeseries.json" &
METRICS_PID=$!

cleanup() {
    log "Stopping metrics collection..."
    kill $METRICS_PID 2>/dev/null || true
    wait $METRICS_PID 2>/dev/null || true
    # Clean up workloads
    kubectl delete deployment -l kubesim-scenario="$SCENARIO" --ignore-not-found 2>/dev/null || true
    log "Done. Results in $RESULTS_DIR/"
}
trap cleanup EXIT

case "$SCENARIO" in
    smoke-test)
        log "=== Smoke Test (2 min) ==="

        # Scale up: 5 pods, 950m CPU each
        log "Creating 5-pod deployment..."
        kubectl apply -f - <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: smoke-workload
  labels:
    kubesim-scenario: smoke-test
spec:
  replicas: 5
  selector:
    matchLabels:
      app: smoke-workload
  template:
    metadata:
      labels:
        app: smoke-workload
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 950m
            memory: 3Gi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOF

        log "Waiting 60s for provisioning..."
        sleep 60

        # Scale down to 1
        log "Scaling down to 1 replica..."
        kubectl scale deployment smoke-workload --replicas=1

        log "Waiting 60s for consolidation..."
        sleep 60

        log "Smoke test complete"
        ;;

    benchmark-control)
        log "=== Benchmark Control (40 min) ==="

        # Phase 1: Start with 1 pod per deployment (2 total)
        log "Phase 1: Creating 2 deployments with 1 replica each..."

        # Deployment A: 950m CPU, 3.5Gi memory
        kubectl apply -f - <<'EOFA'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: workload-a
  labels:
    kubesim-scenario: benchmark-control
spec:
  replicas: 1
  selector:
    matchLabels:
      app: workload-a
  template:
    metadata:
      labels:
        app: workload-a
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 950m
            memory: 3.5Gi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOFA

        # Deployment B: 950m CPU, 6.5Gi memory
        kubectl apply -f - <<'EOFB'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: workload-b
  labels:
    kubesim-scenario: benchmark-control
spec:
  replicas: 1
  selector:
    matchLabels:
      app: workload-b
  template:
    metadata:
      labels:
        app: workload-b
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 950m
            memory: 6.5Gi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOFB

        # Phase 2: Scale out at t=10s
        log "Waiting 10s then scaling to 500 replicas each..."
        sleep 10
        kubectl scale deployment workload-a --replicas=500
        kubectl scale deployment workload-b --replicas=500
        log "Scale-out triggered (1000 total pods)"

        # Phase 3: Scale down at t=15m
        remaining=$((15*60 - 10))
        log "Waiting ${remaining}s until t=15m..."
        sleep $remaining
        log "Phase 3: Scaling down by 150 each (500→350)..."
        kubectl scale deployment workload-a --replicas=350
        kubectl scale deployment workload-b --replicas=350

        # Phase 4: Scale down at t=25m
        log "Waiting 10m until t=25m..."
        sleep $((10*60))
        log "Phase 4: Scaling down by 340 each (350→10)..."
        kubectl scale deployment workload-a --replicas=10
        kubectl scale deployment workload-b --replicas=10

        # Stabilization: wait until t=40m
        log "Waiting 15m for consolidation stabilization..."
        sleep $((15*60))

        log "Benchmark control complete"
        ;;

    *)
        echo "Unknown scenario: $SCENARIO"
        echo "Usage: $0 {smoke-test|benchmark-control}"
        exit 1
        ;;
esac
