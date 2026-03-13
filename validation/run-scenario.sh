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

    adversarial-churn)
        log "=== Adversarial Churn: Rapid scale-up/down (5 min) ==="

        kubectl apply -f - <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: churn-workload
  labels:
    kubesim-scenario: adversarial-churn
spec:
  replicas: 50
  selector:
    matchLabels:
      app: churn-workload
  template:
    metadata:
      labels:
        app: churn-workload
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 500m
            memory: 512Mi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOF
        log "Created 50 pods (500m CPU each)"

        # t=30s: scale to 200
        sleep 30
        log "t=30s: Scaling to 200 replicas..."
        kubectl scale deployment churn-workload --replicas=200

        # t=90s: scale down to 20
        sleep 60
        log "t=90s: Scaling down to 20 replicas..."
        kubectl scale deployment churn-workload --replicas=20

        # t=150s: scale to 150
        sleep 60
        log "t=150s: Scaling up to 150 replicas..."
        kubectl scale deployment churn-workload --replicas=150

        # t=210s: scale down to 10
        sleep 60
        log "t=210s: Scaling down to 10 replicas..."
        kubectl scale deployment churn-workload --replicas=10

        # Wait until t=300s for stabilization
        sleep 90
        log "t=300s: Stabilization complete"
        ;;

    adversarial-heterogeneous)
        log "=== Adversarial Heterogeneous: Mixed pod sizes (10 min) ==="

        # Tiny pods: 20 × 100m CPU, 128Mi
        kubectl apply -f - <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: hetero-tiny
  labels:
    kubesim-scenario: adversarial-heterogeneous
spec:
  replicas: 20
  selector:
    matchLabels:
      app: hetero-tiny
  template:
    metadata:
      labels:
        app: hetero-tiny
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 100m
            memory: 128Mi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOF

        # Medium pods: 10 × 500m CPU, 2Gi
        kubectl apply -f - <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: hetero-medium
  labels:
    kubesim-scenario: adversarial-heterogeneous
spec:
  replicas: 10
  selector:
    matchLabels:
      app: hetero-medium
  template:
    metadata:
      labels:
        app: hetero-medium
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 500m
            memory: 2Gi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOF

        # Large pods: 5 × 2000m CPU, 8Gi
        kubectl apply -f - <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: hetero-large
  labels:
    kubesim-scenario: adversarial-heterogeneous
spec:
  replicas: 5
  selector:
    matchLabels:
      app: hetero-large
  template:
    metadata:
      labels:
        app: hetero-large
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: "2000m"
            memory: 8Gi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOF
        log "Created 35 pods (20 tiny + 10 medium + 5 large)"

        # t=3m: scale down tiny to 5, add 10 more large
        sleep 180
        log "t=3m: Scaling tiny 20→5, large 5→15..."
        kubectl scale deployment hetero-tiny --replicas=5
        kubectl scale deployment hetero-large --replicas=15

        # t=6m: scale down all to minimum
        sleep 180
        log "t=6m: Scaling all to minimum (1 each)..."
        kubectl scale deployment hetero-tiny --replicas=1
        kubectl scale deployment hetero-medium --replicas=1
        kubectl scale deployment hetero-large --replicas=1

        # Wait until t=10m
        sleep 240
        log "t=10m: Stabilization complete"
        ;;

    adversarial-deletion-cost)
        log "=== Adversarial Deletion Cost: Node drain ordering (15 min) ==="

        kubectl apply -f - <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: deletion-cost-workload
  labels:
    kubesim-scenario: adversarial-deletion-cost
spec:
  replicas: 100
  selector:
    matchLabels:
      app: deletion-cost-workload
  template:
    metadata:
      labels:
        app: deletion-cost-workload
    spec:
      terminationGracePeriodSeconds: 0
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: 500m
            memory: 512Mi
      tolerations:
      - key: "kwok.x-k8s.io/node"
        operator: "Exists"
        effect: "NoSchedule"
EOF
        log "Created 100 pods (500m CPU each)"

        # t=5m: scale down to 50
        sleep 300
        log "t=5m: Scaling down to 50 replicas..."
        kubectl scale deployment deletion-cost-workload --replicas=50

        # t=10m: scale down to 10
        sleep 300
        log "t=10m: Scaling down to 10 replicas..."
        kubectl scale deployment deletion-cost-workload --replicas=10

        # Wait until t=15m
        sleep 300
        log "t=15m: Stabilization complete"
        ;;

    *)
        echo "Unknown scenario: $SCENARIO"
        echo "Usage: $0 {smoke-test|benchmark-control|adversarial-churn|adversarial-heterogeneous|adversarial-deletion-cost}"
        exit 1
        ;;
esac
