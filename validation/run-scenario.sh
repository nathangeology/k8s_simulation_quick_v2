#!/usr/bin/env bash
# run-scenario.sh — Apply workload manifests to a KIND cluster and collect metrics
#
# Translates a kubesim scenario YAML to K8s manifests, applies them,
# collects time-series metrics for the scenario duration, then exports results.
#
# Usage:
#   ./validation/run-scenario.sh <scenario.yaml> [--context CTX] [--duration SECS] [--output DIR]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

CONTEXT="kind-kubesim-val"
DURATION=""
OUTPUT_DIR="${PROJECT_DIR}/validation/results"
INTERVAL=30

usage() {
  echo "Usage: $0 <scenario.yaml> [--context CTX] [--duration SECS] [--output DIR]" >&2
  exit 1
}

[[ $# -lt 1 ]] && usage
SCENARIO="$1"; shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --context)  CONTEXT="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --output)   OUTPUT_DIR="$2"; shift 2 ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    *) echo "Unknown arg: $1" >&2; usage ;;
  esac
done

[[ -f "$SCENARIO" ]] || { echo "ERROR: scenario not found: $SCENARIO" >&2; exit 1; }

SCENARIO_NAME="$(basename "$SCENARIO" .yaml)"
MANIFESTS_DIR="${OUTPUT_DIR}/${SCENARIO_NAME}/manifests"
METRICS_FILE="${OUTPUT_DIR}/${SCENARIO_NAME}/metrics.parquet"

mkdir -p "$MANIFESTS_DIR"

# Step 1: Translate scenario to K8s manifests
echo "==> Translating ${SCENARIO} to manifests"
cd "$PROJECT_DIR"
PYTHONPATH="${PROJECT_DIR}/python" python -c "
from kubesim.validation.translator import translate_scenario
written = translate_scenario('${SCENARIO}', '${MANIFESTS_DIR}')
print(f'  Generated {len(written)} manifests')
"

# Step 2: Apply workload manifests only (skip NodePool/EC2NodeClass — cluster setup handles those)
echo "==> Applying workload manifests to ${CONTEXT}"
for f in "$MANIFESTS_DIR"/*.yaml; do
  base="$(basename "$f")"
  case "$base" in
    nodepool-*|ec2nodeclass-*) echo "  Skipping $base (cluster-managed)" ;;
    *) kubectl --context "$CONTEXT" apply -f "$f" 2>&1 | tail -1 ;;
  esac
done

# Step 3: Auto-detect duration from scenario if not specified
if [[ -z "$DURATION" ]]; then
  DURATION=$(PYTHONPATH="${PROJECT_DIR}/python" python -c "
import yaml
with open('${SCENARIO}') as f:
    s = yaml.safe_load(f)
study = s.get('study', s)
d = study.get('duration', '2m')
if isinstance(d, str):
    if d.endswith('m'): print(int(d[:-1]) * 60)
    elif d.endswith('s'): print(int(d[:-1]))
    else: print(120)
else:
    print(int(d))
" 2>/dev/null || echo 120)
  echo "==> Auto-detected duration: ${DURATION}s"
fi

# Step 4: Collect metrics
echo "==> Collecting metrics for ${DURATION}s (interval=${INTERVAL}s)"
PYTHONPATH="${PROJECT_DIR}/python" python -m kubesim.validation.metrics_collector \
  --context "$CONTEXT" \
  --duration "$DURATION" \
  --interval "$INTERVAL" \
  --output "$METRICS_FILE"

echo "==> Scenario complete: ${METRICS_FILE}"
