# Validation: KIND+KWOK+Karpenter 1.9

Reproducible validation cluster for comparing kubesim against real Karpenter behavior.

## Prerequisites

- `kind` >= 0.20.0
- `kubectl` >= 1.28
- `helm` >= 3.12
- Docker running
- Python 3.11+ with `pyyaml`, `polars` (or `pyarrow`)

## Quick Start (Smoke Test)

```bash
# 1. Create cluster
./validation/setup-cluster.sh

# 2. Run smoke test scenario (2 min)
./validation/run-scenario.sh validation/smoke-test.yaml

# 3. Compare with sim results
python validation/compare-results.py \
  results/benchmark-control/benchmark-control/report.json \
  validation/results/smoke-test/metrics.parquet \
  --output validation/results/comparison.json

# 4. Teardown
./validation/teardown-cluster.sh
```

## Scripts

| Script | Purpose |
|--------|---------|
| `setup-cluster.sh` | Creates KIND cluster, installs KWOK + Karpenter 1.9 |
| `teardown-cluster.sh` | Destroys the KIND cluster |
| `collect-metrics.sh` | Runs metrics collection in background |
| `run-scenario.sh` | Translates scenario, applies manifests, collects metrics |
| `compare-results.py` | Compares sim report.json with real metrics, produces fidelity report |
| `smoke-test.yaml` | Minimal 2-minute validation scenario |

## Cluster Configuration

- Karpenter 1.9 (Helm chart 1.1.1)
- KWOK NodeClass (fake nodes, no EC2)
- NodePool: m5.xlarge (4 vCPU, 16 GiB) + m5.2xlarge (8 vCPU, 32 GiB)
- Max 200 nodes, WhenUnderutilized consolidation
- KWOK nodes report realistic allocatable resources matching EKS overhead table
