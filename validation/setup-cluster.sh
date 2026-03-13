#!/bin/bash
set -euo pipefail

# KIND + KWOK + Karpenter cluster for kubesim validation
# Based on https://github.com/dacort/kinda-yunikarp (stripped to essentials)

CLUSTER_NAME=${CLUSTER_NAME:-kubesim-val}
KARPENTER_VERSION=${KARPENTER_VERSION:-v1.1.0}
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export PATH="$HOME/go/bin:$PATH"
WORK_DIR="${SCRIPT_DIR}/.work"

log() { echo "[$(date +%H:%M:%S)] $1"; }

mkdir -p "$WORK_DIR"

# Step 1: KIND cluster
if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    log "Cluster $CLUSTER_NAME exists, reusing"
    kind export kubeconfig --name "$CLUSTER_NAME"
else
    log "Creating KIND cluster: $CLUSTER_NAME"
    cat <<EOF | kind create cluster --name "$CLUSTER_NAME" --config=-
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
- role: control-plane
- role: worker
EOF
fi

kubectl wait --for=condition=Ready node --all --timeout=120s
log "Cluster ready"

# Step 2: Install KWOK
if ! kubectl get deployment kwok-controller -n kube-system &>/dev/null; then
    log "Installing KWOK controller..."
    KWOK_REPO=kubernetes-sigs/kwok
    KWOK_RELEASE=$(curl -s "https://api.github.com/repos/${KWOK_REPO}/releases/latest" | jq -r '.tag_name')
    kubectl apply -f "https://github.com/${KWOK_REPO}/releases/download/${KWOK_RELEASE}/kwok.yaml"
    kubectl apply -f "https://github.com/${KWOK_REPO}/releases/download/${KWOK_RELEASE}/stage-fast.yaml"
    kubectl wait --for=condition=available deployment/kwok-controller -n kube-system --timeout=120s
    log "KWOK installed"
else
    log "KWOK already installed"
fi

# Step 3: Clone and build Karpenter with KWOK provider
if ! kubectl get deployment karpenter -n kube-system &>/dev/null; then
    log "Building Karpenter ${KARPENTER_VERSION} with KWOK provider..."

    if [ ! -d "$WORK_DIR/karpenter" ]; then
        git clone https://github.com/kubernetes-sigs/karpenter.git "$WORK_DIR/karpenter"
    fi

    cd "$WORK_DIR/karpenter"
    git fetch --tags
    git switch --detach "$KARPENTER_VERSION" 2>/dev/null || git checkout "$KARPENTER_VERSION"

    # Install Prometheus (Karpenter dependency)
    log "Installing Prometheus..."
    ./hack/install-prometheus.sh

    # Build and deploy with KWOK provider (skip verify — needs extra tools)
    log "Building Karpenter (this takes a few minutes)..."
    export KWOK_REPO=kind.local
    export KIND_CLUSTER_NAME="$CLUSTER_NAME"
    make build-with-kind
    # Apply CRDs and helm install manually (same as apply-with-kind minus verify)
    kubectl apply -f kwok/charts/crds
    helm upgrade --install karpenter kwok/charts --namespace kube-system --skip-crds \
        --set controller.image.repository=$(echo $CONTROLLER_IMG | cut -d: -f1) \
        --set controller.image.tag=latest \
        --set serviceMonitor.enabled=false

    kubectl wait --for=condition=available deployment/karpenter -n kube-system --timeout=300s
    log "Karpenter installed"
    cd "$SCRIPT_DIR"
else
    log "Karpenter already installed"
fi

# Step 4: Configure instance types
log "Configuring instance types..."
kubectl create configmap -n kube-system karpenter-instance-types \
    --from-file="${SCRIPT_DIR}/instance-types.json" \
    --dry-run=client -o yaml | kubectl apply -f -

# Patch Karpenter to use custom instance types
kubectl patch deployment karpenter -n kube-system --type=strategic --patch '
spec:
  template:
    spec:
      containers:
      - name: controller
        env:
        - name: KWOK_INSTANCE_TYPES_PATH
          value: /etc/karpenter/instance-types.json
        volumeMounts:
        - name: instance-types
          mountPath: /etc/karpenter
          readOnly: true
      volumes:
      - name: instance-types
        configMap:
          name: karpenter-instance-types
' 2>/dev/null || true

kubectl rollout status deployment/karpenter -n kube-system --timeout=120s

# Step 5: Configure NodePool + NodeClass
log "Configuring NodePool and KWOKNodeClass..."
cat <<EOF | kubectl apply -f -
apiVersion: karpenter.kwok.sh/v1alpha1
kind: KWOKNodeClass
metadata:
  name: default
---
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: default
spec:
  template:
    spec:
      requirements:
        - key: kubernetes.io/arch
          operator: In
          values: ["amd64"]
        - key: kubernetes.io/os
          operator: In
          values: ["linux"]
        - key: karpenter.sh/capacity-type
          operator: In
          values: ["on-demand"]
        - key: node.kubernetes.io/instance-type
          operator: In
          values: ["m5.xlarge", "m5.2xlarge"]
      nodeClassRef:
        name: default
        kind: KWOKNodeClass
        group: karpenter.kwok.sh
      expireAfter: 720h
  limits:
    cpu: "800"
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized
    consolidateAfter: 30s
EOF

log "=== Cluster ready for validation ==="
log "Nodes: $(kubectl get nodes --no-headers | wc -l)"
log "Karpenter: $(kubectl get pods -n kube-system -l app.kubernetes.io/name=karpenter --no-headers)"
