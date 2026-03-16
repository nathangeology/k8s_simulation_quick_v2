#!/bin/bash
set -euo pipefail
CLUSTER_NAME=${CLUSTER_NAME:-kubesim-val}
echo "Deleting KIND cluster: $CLUSTER_NAME"
kind delete cluster --name "$CLUSTER_NAME"
echo "Done"
