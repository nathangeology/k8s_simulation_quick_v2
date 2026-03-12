#!/usr/bin/env bash
# kind-karpenter-setup.sh — Create/delete a KIND cluster with KWOK + Karpenter + kube-state-metrics
#
# Prerequisites:
#   - kind >= 0.20.0        (https://kind.sigs.k8s.io/)
#   - kubectl >= 1.28       (https://kubernetes.io/docs/tasks/tools/)
#   - helm >= 3.12          (https://helm.sh/docs/intro/install/)
#   - docker running
#
# Usage:
#   ./scripts/kind-karpenter-setup.sh create   # Create cluster with all components
#   ./scripts/kind-karpenter-setup.sh delete   # Tear down cluster

set -euo pipefail

CLUSTER_NAME="${KIND_CLUSTER_NAME:-kubesim}"
KARPENTER_NAMESPACE="kube-system"
KARPENTER_VERSION="${KARPENTER_VERSION:-1.1.1}"
KSM_VERSION="${KSM_VERSION:-5.27.0}"

log() { echo "==> $*"; }
err() { echo "ERROR: $*" >&2; exit 1; }

check_prereqs() {
  local missing=()
  command -v kind    >/dev/null || missing+=(kind)
  command -v kubectl >/dev/null || missing+=(kubectl)
  command -v helm    >/dev/null || missing+=(helm)
  command -v docker  >/dev/null || missing+=(docker)
  if [[ ${#missing[@]} -gt 0 ]]; then
    err "Missing prerequisites: ${missing[*]}"
  fi
  docker info >/dev/null 2>&1 || err "Docker is not running"
}

create_cluster() {
  check_prereqs

  if kind get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
    log "Cluster '$CLUSTER_NAME' already exists, skipping creation"
  else
    log "Creating KIND cluster '$CLUSTER_NAME'"
    kind create cluster --name "$CLUSTER_NAME" --wait 60s
  fi

  kubectl cluster-info --context "kind-${CLUSTER_NAME}" >/dev/null || err "Cannot reach cluster"

  # Install KWOK
  log "Installing KWOK controller"
  local kwok_repo="kubernetes-sigs/kwok"
  local kwok_release
  kwok_release=$(curl -s "https://api.github.com/repos/${kwok_repo}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
  if [[ -z "$kwok_release" ]]; then
    kwok_release="v0.7.0"
    log "Could not fetch latest KWOK release, using ${kwok_release}"
  fi

  kubectl apply -f "https://github.com/${kwok_repo}/releases/download/${kwok_release}/kwok.yaml" 2>&1 | tail -1
  kubectl apply -f "https://github.com/${kwok_repo}/releases/download/${kwok_release}/stage-fast.yaml" 2>&1 | tail -1

  log "Waiting for KWOK controller to be ready"
  kubectl wait --for=condition=Available -n kube-system deployment/kwok-controller --timeout=120s

  # Install Karpenter with KWOK cloud provider
  log "Installing Karpenter ${KARPENTER_VERSION} with KWOK provider"
  helm upgrade --install karpenter oci://public.ecr.aws/karpenter/karpenter \
    --version "$KARPENTER_VERSION" \
    --namespace "$KARPENTER_NAMESPACE" \
    --set "settings.clusterName=${CLUSTER_NAME}" \
    --set "settings.isolatedVPC=true" \
    --set "controller.image.repository=public.ecr.aws/karpenter/karpenter" \
    --set "controller.image.tag=${KARPENTER_VERSION}" \
    --set "replicas=1" \
    --wait --timeout 120s 2>&1 | tail -1

  # Apply KWOK NodeClass so Karpenter can provision via KWOK
  log "Creating default KWOK NodeClass and NodePool"
  kubectl apply -f - <<'EOF'
apiVersion: karpenter.kwok.sh/v1alpha1
kind: KWOKNodeClass
metadata:
  name: default
EOF

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
        - key: kubernetes.io/arch
          operator: In
          values: ["amd64"]
        - key: karpenter.sh/capacity-type
          operator: In
          values: ["on-demand"]
  limits:
    cpu: "1000"
    memory: 1000Gi
  disruption:
    consolidationPolicy: WhenEmptyOrUnderutilized
    consolidateAfter: 30s
EOF

  # Install kube-state-metrics
  log "Installing kube-state-metrics"
  helm repo add prometheus-community https://prometheus-community.github.io/helm-charts 2>/dev/null || true
  helm repo update prometheus-community 2>/dev/null
  helm upgrade --install kube-state-metrics prometheus-community/kube-state-metrics \
    --version "$KSM_VERSION" \
    --namespace kube-system \
    --wait --timeout 60s 2>&1 | tail -1

  # Verify
  log "Verifying setup"
  kubectl get nodepools
  kubectl get deployment -n "$KARPENTER_NAMESPACE" -l app.kubernetes.io/name=karpenter

  log "Cluster '$CLUSTER_NAME' is ready"
}

delete_cluster() {
  if kind get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
    log "Deleting KIND cluster '$CLUSTER_NAME'"
    kind delete cluster --name "$CLUSTER_NAME"
    log "Cluster deleted"
  else
    log "Cluster '$CLUSTER_NAME' does not exist"
  fi
}

case "${1:-}" in
  create) create_cluster ;;
  delete) delete_cluster ;;
  *)
    echo "Usage: $0 {create|delete}" >&2
    exit 1
    ;;
esac
