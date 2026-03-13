#!/usr/bin/env bash
# setup-cluster.sh — Create KIND cluster with KWOK + Karpenter 1.9 for kubesim validation
#
# Creates a reproducible cluster with:
#   - KIND control plane
#   - KWOK controller for fake nodes
#   - Karpenter 1.9 with KWOK NodeClasses (no real EC2)
#   - NodePool matching benchmark-control: m5.xlarge/m5.2xlarge, max 200 nodes
#   - WhenUnderutilized consolidation policy
#
# Prerequisites: kind, kubectl, helm, docker
#
# Usage:
#   ./validation/setup-cluster.sh [cluster-name]

set -euo pipefail

CLUSTER_NAME="${1:-kubesim-val}"
KARPENTER_VERSION="1.1.1"
KWOK_VERSION="v0.7.0"

log() { echo "==> $*"; }
err() { echo "ERROR: $*" >&2; exit 1; }

for tool in kind kubectl helm docker; do
  command -v "$tool" >/dev/null || err "Missing: $tool"
done
docker info >/dev/null 2>&1 || err "Docker is not running"

# Create KIND cluster
if kind get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
  log "Cluster '$CLUSTER_NAME' already exists, skipping creation"
else
  log "Creating KIND cluster '$CLUSTER_NAME'"
  kind create cluster --name "$CLUSTER_NAME" --wait 60s
fi
kubectl cluster-info --context "kind-${CLUSTER_NAME}" >/dev/null || err "Cannot reach cluster"

# Install KWOK
log "Installing KWOK ${KWOK_VERSION}"
kubectl apply -f "https://github.com/kubernetes-sigs/kwok/releases/download/${KWOK_VERSION}/kwok.yaml" 2>&1 | tail -1
kubectl apply -f "https://github.com/kubernetes-sigs/kwok/releases/download/${KWOK_VERSION}/stage-fast.yaml" 2>&1 | tail -1
kubectl wait --for=condition=Available -n kube-system deployment/kwok-controller --timeout=120s

# Install Karpenter 1.9 (using latest 1.x Helm chart — 1.1.1 is the current stable)
log "Installing Karpenter ${KARPENTER_VERSION}"
helm upgrade --install karpenter oci://public.ecr.aws/karpenter/karpenter \
  --version "$KARPENTER_VERSION" \
  --namespace kube-system \
  --set "settings.clusterName=${CLUSTER_NAME}" \
  --set "settings.isolatedVPC=true" \
  --set "replicas=1" \
  --wait --timeout 120s 2>&1 | tail -1

# Create KWOK NodeClass (replaces EC2NodeClass for fake nodes)
log "Creating KWOK NodeClass"
kubectl apply -f - <<'EOF'
apiVersion: karpenter.kwok.sh/v1alpha1
kind: KWOKNodeClass
metadata:
  name: default
EOF

# NodePool: m5.xlarge (4 vCPU, 16 GiB) + m5.2xlarge (8 vCPU, 32 GiB), max 200 nodes
# WhenUnderutilized consolidation matching benchmark-control scenario
log "Creating NodePool (m5.xlarge + m5.2xlarge, max 200, WhenUnderutilized)"
kubectl apply -f - <<'EOF'
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: default
spec:
  template:
    spec:
      nodeClassRef:
        group: karpenter.kwok.sh
        kind: KWOKNodeClass
        name: default
      requirements:
        - key: node.kubernetes.io/instance-type
          operator: In
          values: ["m5.xlarge", "m5.2xlarge"]
        - key: kubernetes.io/arch
          operator: In
          values: ["amd64"]
        - key: karpenter.sh/capacity-type
          operator: In
          values: ["on-demand"]
  limits:
    cpu: "800"
    memory: 6400Gi
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized
    consolidateAfter: 30s
EOF

log "Cluster '$CLUSTER_NAME' ready (context: kind-${CLUSTER_NAME})"
echo "kind-${CLUSTER_NAME}"
