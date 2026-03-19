# KubeSim — Kubernetes Cluster Simulator

Fast discrete-event simulator for evaluating Kubernetes scheduling, autoscaling,
and pod lifecycle strategies at scale. Rust core with Python orchestration layer.

## Quick Start

```bash
# Build Rust core + Python bindings
maturin develop --release

# Run a study (A/B comparison with report)
python -m kubesim report scenarios/scheduling-comparison.yaml --seeds 10

# Run adversarial search
python -m kubesim run-adversarial --budget 200 --variants scoring

# Run the full Optuna-based adversarial finder
python scripts/find_adversarial.py --budget 100 --top-k 10
```

## Prerequisites

- Rust toolchain (stable)
- Python ≥ 3.9
- [maturin](https://github.com/PyO3/maturin) (`pip install maturin`)

```bash
# Create venv and install dependencies
python -m venv .venv
source .venv/bin/activate
pip install maturin pyyaml optuna numpy scipy polars matplotlib

# Build native extension into the venv
maturin develop --release
```

## Commands

### `kubesim report` — Run a study and generate A/B comparison

Runs all variants defined in a scenario YAML, collects metrics across seeds,
and produces a statistical comparison report with plots.

```bash
python -m kubesim report <scenario.yaml> [--seeds N] [--output-dir DIR]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--seeds` | 5 | Number of random seeds to run per variant |
| `--output-dir` | `results/` | Output directory for report.json, report.md, and plots |

Output:
- `results/<study-name>/report.json` — Machine-readable metrics and comparisons
- `results/<study-name>/report.md` — Human-readable Markdown with tables
- `results/<study-name>/ts_*.png` — Timeseries plots (cost, pods, nodes over time)
- `results/<study-name>/dist_*.png` — Distribution box plots across seeds

Example:
```bash
python -m kubesim report scenarios/deletion-cost-drain.yaml --seeds 20
cat results/deletion-cost-node-drain/report.md
```

### `kubesim run-adversarial` — Hypothesis-based adversarial search

Searches the scenario space for configurations where two strategy variants
diverge most, using Hypothesis for coverage-guided exploration.

```bash
python -m kubesim run-adversarial [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--budget` | 1000 | Maximum scenario evaluations |
| `--top-k` | 10 | Number of worst-case scenarios to save |
| `--seed` | 0 | RNG seed for reproducibility |
| `--variants` | `scoring` | Variant pair: `scoring`, `karpenter`, or `deletion_cost` |
| `--chaos` | off | Enable chaos mode (extreme configs) |
| `--objectives` | none | Additional objectives: `cost_efficiency`, `availability`, `consolidation_waste`, `disruption_rate`, `scheduling_failure_rate`, `entropy_deviation` |
| `--track-features` | off | Report which config dimensions drive divergence |
| `--outdir` | `scenarios/adversarial` | Where to save worst-case YAML files |

Variant pairs:
- `scoring` — MostAllocated vs LeastAllocated scheduling
- `karpenter` — WhenEmpty vs WhenUnderutilized consolidation
- `deletion_cost` — No deletion cost vs prefer-emptying-nodes draining

#### Scheduling strategy divergence (MostAllocated vs LeastAllocated)

Finds cluster configurations where bin-packing vs spreading produces the
largest cost or utilization difference.

```bash
# Quick exploration (a few minutes)
python -m kubesim run-adversarial --budget 200 --variants scoring --top-k 10

# Deep search with feature importance tracking
python -m kubesim run-adversarial --budget 1000 --variants scoring --track-features

# Chaos mode — single-instance pools, overcommit, extreme replicas
python -m kubesim run-adversarial --budget 500 --variants scoring --chaos
```

#### Karpenter consolidation policy (WhenEmpty vs WhenUnderutilized)

Finds scenarios where the two consolidation policies diverge — useful for
understanding when WhenUnderutilized's aggressive node removal helps or hurts.

```bash
# Standard search
python -m kubesim run-adversarial --budget 500 --variants karpenter

# With disruption and availability tracking
python -m kubesim run-adversarial --budget 500 --variants karpenter \
    --objectives disruption_rate availability consolidation_waste
```

#### Pod deletion cost strategy (none vs prefer-emptying-nodes)

Finds scenarios where deletion cost annotations change which pods get evicted
during scale-down, affecting availability and disruption patterns.

```bash
# Standard search
python -m kubesim run-adversarial --budget 300 --variants deletion_cost

# Focus on availability and disruption impact
python -m kubesim run-adversarial --budget 500 --variants deletion_cost \
    --objectives availability disruption_rate
```

#### Instance type selection

By default, ~80% of generated scenarios use all available EC2 instance types
(the realistic case), while ~20% use a restricted subset to explore
constrained bin-packing scenarios. Chaos mode always uses single-instance
pools for maximum stress.

### Standalone Adversarial Scripts

More targeted searches with Optuna TPE (Bayesian optimization), detailed
categorization, and multi-objective ranking.

```bash
# MostAllocated vs LeastAllocated — categorized by direction
python scripts/find_adversarial.py --budget 100 --top-k 10

# 5-way deletion cost strategy comparison
python scripts/find_adversarial_deletion_cost.py

# Karpenter v0.35 vs v1.x version comparison
python scripts/find_adversarial_karpenter_version.py
```

These scripts write:
- `scenarios/adversarial/<category>/*.yaml` — Discovered worst-case scenarios
- `results/adversarial/manifest.json` — Scores, categories, per-variant metrics

### `kubesim translate` — Convert scenarios to K8s manifests

Translates a KubeSim scenario YAML into real Kubernetes manifests for
validation on KIND/KWOK or EKS clusters.

```bash
python -m kubesim translate <scenario.yaml> [-o manifests/] [--variant NAME]
```

### `kubesim validate-kwok` — Run on KIND + KWOK cluster

Deploys translated manifests on a local KIND cluster with KWOK fake nodes
and collects real scheduling metrics.

```bash
python -m kubesim validate-kwok <manifests-dir/> [-o results.parquet] [--nodes 10] [--settle 30]
```

### `kubesim compare` — Cross-tier divergence report

Compares results across simulation tiers (Tier 1 sim vs Tier 2 KWOK vs Tier 3 EKS).

```bash
python -m kubesim compare tier1.parquet tier2.parquet --threshold 0.05 -o report.html
```

## Scenario YAML Format

Scenarios define the cluster, workloads, variants to compare, and metrics to collect.

```yaml
study:
  name: my-experiment
  runs: 1000              # seeds per variant
  time_mode: wall_clock   # wall_clock (realistic durations) or logical (max speed)

  cluster:
    node_pools:
      - instance_types: [m5.xlarge, m5.2xlarge, c5.xlarge]
        min_nodes: 3
        max_nodes: 100
        karpenter:
          consolidation: {policy: WhenUnderutilized}

  workloads:
    - type: web_app
      count: 10
      replicas: {min: 5, max: 50}
      scaling: {type: hpa, metric: cpu, target: "70%"}
      pdb: {min_available: "50%"}
      topology_spread: {max_skew: 1, topology_key: topology.kubernetes.io/zone}
    - type: batch_job
      count: {dist: poisson, lambda: 5}
      priority: low

  traffic_pattern:                    # optional
    type: diurnal_with_spike          # diurnal, spike, steady, diurnal_with_spike
    peak_multiplier: 5
    duration: 24h

  variants:
    - name: most_allocated
      scheduler: {scoring: MostAllocated, weight: 1}
    - name: least_allocated
      scheduler: {scoring: LeastAllocated, weight: 1}

  metrics:
    compare: [total_cost, disruption_count, p99_scheduling_latency]
```

### Workload Types

| Type | Description |
|------|-------------|
| `web_app` | Long-running, HPA-scaled, diurnal traffic |
| `saas_microservice` | High-replica, topology-spread, PDB-protected |
| `batch_job` | Short-lived, parallelized, low priority |
| `ml_training` | GPU-hungry, single replica, high priority |

### Variant Options

Variants override cluster/scheduler config for A/B comparison:

```yaml
# Scheduling strategy
scheduler: {scoring: MostAllocated, weight: 1}   # or LeastAllocated

# Karpenter consolidation
karpenter: {consolidation: {policy: WhenEmpty}}   # or WhenUnderutilized

# Deletion cost strategy
deletion_cost_strategy: none                       # none, prefer_emptying_nodes,
                                                   # largest_first, unallocated_vcpu, random

# Karpenter version
karpenter_version: "v0.35"                         # or "v1"
```

## Python API

```python
import kubesim

# Single simulation run
sim = kubesim.Simulation(config="scenarios/scheduling-comparison.yaml",
                         time_mode="logical", seed=42)
result = sim.run()
print(result.total_cost_per_hour, result.node_count, result.pending_pods)

# Large-scale run with higher event budget (default 10M)
sim = kubesim.Simulation(config="scenarios/large-cluster.yaml",
                         time_mode="logical", seed=42,
                         event_budget=50_000_000)
result = sim.run()

# Parallel batch run across seeds
results = kubesim.batch_run(config_yaml, seeds=[0, 1, 2, 3, 4])

# Batch run with custom event budget for large scenarios
results = kubesim.batch_run(config_yaml, seeds=[0, 1, 2],
                            event_budget=100_000_000)

# Adversarial search (Hypothesis-based)
from kubesim import AdversarialFinder, ScenarioSpace, MOST_VS_LEAST
finder = AdversarialFinder(
    objective="maximize",
    metric=lambda results: abs(
        sum(r["total_cost_per_hour"] for r in results if r["variant"] == "most_allocated") -
        sum(r["total_cost_per_hour"] for r in results if r["variant"] == "least_allocated")
    ),
    space=ScenarioSpace(nodes=(10, 500)),
    variant_pair=MOST_VS_LEAST,
    budget=200,
    seeds=[42, 100, 200],
)
worst_cases = finder.run()

# Adversarial search (Optuna TPE)
from kubesim import OptunaAdversarialSearch, KARPENTER_CONSOLIDATION
search = OptunaAdversarialSearch(
    objective_fn=lambda results: abs(
        sum(r["total_cost_per_hour"] for r in results if r["variant"] == "when_empty") -
        sum(r["total_cost_per_hour"] for r in results if r["variant"] == "when_underutilized")
    ),
    variant_pair=KARPENTER_CONSOLIDATION,
    budget=200,
    seeds=[42, 100, 200],
)
worst_cases = search.run()

# Analysis
from kubesim.analysis import results_to_df, compare_variants, mann_whitney, bootstrap_ci
df = results_to_df(results)
print(compare_variants(df))
print(mann_whitney(df, "most_allocated", "least_allocated", "cumulative_cost"))
```

### Event Budget

Each simulation run has an event budget (default 10 million) that caps the
maximum number of discrete events processed. This prevents runaway scenarios
from hanging indefinitely — particularly useful during adversarial search
where randomly generated scenarios can produce pathological event counts.

For large-scale deliberate runs (10K+ nodes, 100K+ pods), increase the budget:

```python
# Via Simulation
sim = kubesim.Simulation(yaml, event_budget=100_000_000)

# Via batch_run
results = kubesim.batch_run(yaml, seeds, event_budget=100_000_000)
```

When a run hits the budget, it stops early and returns partial results. You
can detect this by checking `events_processed` — if it's close to the budget
value, the simulation was truncated.

## Pre-built Scenarios

| File | What it tests |
|------|---------------|
| `scheduling-comparison.yaml` | MostAllocated vs LeastAllocated with mixed workloads |
| `deletion-cost-drain.yaml` | Pod deletion cost strategies during scale-in |
| `deletion-cost-ranking.yaml` | 5-way deletion cost strategy comparison |
| `karpenter-version-comparison.yaml` | Karpenter v0.35 vs v1.x behavior differences |
| `disruption-budget-stress.yaml` | Disruption budget enforcement under pressure |
| `multi-nodepool.yaml` | Cross-pool scheduling with heterogeneous instances |
| `batch-job-lifecycle.yaml` | Batch job scheduling and preemption |
| `benchmark-control.yaml` | Baseline benchmark for regression testing |

### KWOK Validation Scenarios

Scenarios suffixed with `-kwok.yaml` are designed for Tier 2 validation on
KIND + KWOK clusters:

```bash
# Set up local cluster
bash validation/setup-cluster.sh

# Run a validation scenario
bash validation/run-scenario.sh benchmark-control

# Compare sim vs KWOK results
python -m kubesim compare results/benchmark-control/sim.parquet results/benchmark-control/kwok.parquet

# Tear down
bash validation/teardown-cluster.sh
```

## Objectives (for adversarial search)

| Name | Description | Direction |
|------|-------------|-----------|
| `cost_efficiency` | Total cost per running pod | Lower = better |
| `availability` | Running pods / total requested | Higher = better |
| `consolidation_waste` | 1 − (allocated / allocatable CPU) | Lower = better |
| `disruption_rate` | Evicted pods / total pods | Lower = better |
| `scheduling_failure_rate` | Pending pods / total pods | Lower = better |
| `entropy_deviation` | Distance from uniform node distribution | Lower = better |

## Project Structure

```
├── crates/
│   ├── kubesim-core/        # ClusterState, Node, Pod, Resources (arena-allocated)
│   ├── kubesim-engine/      # DES event loop, logical + wall-clock time modes
│   ├── kubesim-scheduler/   # kube-scheduler filter/score plugin chain
│   ├── kubesim-karpenter/   # Provisioning, consolidation, drift, spot interruption
│   ├── kubesim-ec2/         # EC2 instance type catalog with pricing
│   ├── kubesim-metrics/     # Adaptive metrics collection
│   ├── kubesim-workload/    # Scenario loading and workload generation
│   └── kubesim-py/          # PyO3 bindings (Simulation, batch_run)
├── python/kubesim/
│   ├── __main__.py          # CLI dispatcher
│   ├── adversarial.py       # AdversarialFinder + OptunaAdversarialSearch
│   ├── objectives.py        # Composable objective functions
│   ├── scenario_templates.py # Pre-built parameterized scenario generators
│   ├── strategies.py        # Hypothesis strategies for scenario generation
│   ├── analysis.py          # Polars-based stats (Mann-Whitney, bootstrap CI)
│   ├── report.py            # A/B comparison report generator
│   ├── plots.py             # Timeseries + distribution plots (matplotlib)
│   └── validation/          # Tier 2/3 validation (translate, KWOK, EKS, compare)
├── scenarios/               # Study YAML definitions
│   └── adversarial/         # Discovered worst-case scenarios
├── scripts/                 # Standalone adversarial search scripts
├── results/                 # Output reports, plots, manifests
└── validation/              # KIND/KWOK cluster setup and run scripts
```
