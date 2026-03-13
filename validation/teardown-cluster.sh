#!/usr/bin/env bash
# teardown-cluster.sh — Destroy the validation KIND cluster
#
# Usage:
#   ./validation/teardown-cluster.sh [cluster-name]

set -euo pipefail

CLUSTER_NAME="${1:-kubesim-val}"

if kind get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
  echo "==> Deleting KIND cluster '$CLUSTER_NAME'"
  kind delete cluster --name "$CLUSTER_NAME"
  echo "==> Cluster deleted"
else
  echo "==> Cluster '$CLUSTER_NAME' does not exist"
fi
