#!/usr/bin/env bash
# collect-metrics.sh — Run metrics collection against a KIND cluster
#
# Wraps python -m kubesim.validation.metrics_collector with sensible defaults.
#
# Usage:
#   ./validation/collect-metrics.sh [--context CTX] [--duration SECS] [--output FILE]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

CONTEXT="kind-kubesim-val"
DURATION=120
INTERVAL=30
OUTPUT="${PROJECT_DIR}/validation/metrics.parquet"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --context)  CONTEXT="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    --output)   OUTPUT="$2"; shift 2 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

echo "==> Collecting metrics: context=${CONTEXT} duration=${DURATION}s interval=${INTERVAL}s"
echo "==> Output: ${OUTPUT}"

cd "$PROJECT_DIR"
PYTHONPATH="${PROJECT_DIR}/python" python -m kubesim.validation.metrics_collector \
  --context "$CONTEXT" \
  --duration "$DURATION" \
  --interval "$INTERVAL" \
  --output "$OUTPUT"
