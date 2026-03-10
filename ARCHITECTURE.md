# KubeSim — Kubernetes Cluster Simulator

## Overview

KubeSim is a three-tier Kubernetes cluster simulator for evaluating scheduling,
autoscaling, and pod lifecycle algorithms at scale (up to 100K nodes / 1M pods).

The core insight: most scheduling/autoscaling research requires thousands of
experiment runs, but real clusters and even KWOK simulations are too slow for
rapid iteration. KubeSim provides a fast discrete-event simulation core (Tier 1)
validated against progressively more realistic environments (Tiers 2-3).

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Python Orchestration Layer                   │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌───────────────┐  │
│  │ Scenario │  │    RL    │  │ Property  │  │   Analysis    │  │
│  │ Generator│  │ Training │  │  Testing  │  │ & Reporting   │  │
│  │          │  │(Gymnasium│  │(Hypothesis│  │ (Polars/      │  │
│  │          │  │ + SB3)   │  │ + custom) │  │  Plotly)      │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  └───────┬───────┘  │
│       │              │              │                │           │
│       └──────────────┴──────┬───────┴────────────────┘           │
│                             │                                    │
│                      ┌──────▼──────┐                             │
│                      │  PyO3 FFI   │                             │
│                      └──────┬──────┘                             │
└─────────────────────────────┼───────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│                    Rust Simulation Core                          │
│                                                                  │
│  ┌────────────┐  ┌────────────┐  ┌─────────────┐               │
│  │  Scheduler │  │  Karpenter │  │  Workload   │               │
│  │  Model     │  │  Model     │  │  Generator  │               │
│  │            │  │            │  │             │               │
│  │ - Filters  │  │ - Provisn  │  │ - Traces   │               │
│  │ - Scoring  │  │ - Consolid │  │ - Synthetic│               │
│  │ - Preempt  │  │ - Drift    │  │ - PBT      │               │
│  │ - Topology │  │ - Spot int │  │ - Designed │               │
│  └─────┬──────┘  └──────┬─────┘  └──────┬──────┘               │
│        │                │               │                       │
│  ┌─────▼────────────────▼───────────────▼──────┐                │
│  │            Cluster State Machine             │                │
│  │                                              │                │
│  │  Nodes[]  Pods[]  Events[]  Clock            │                │
│  │  (arena-allocated, cache-friendly)           │                │
│  └──────────────────┬───────────────────────────┘                │
│                     │                                            │
│  ┌──────────────────▼───────────────────────────┐                │
│  │           DES Event Loop                      │                │
│  │  Priority queue of (SimTime, Event)           │                │
│  │  Tick-based OR wall-clock mode                │                │
│  └──────────────────┬───────────────────────────┘                │
│                     │                                            │
│  ┌──────────────────▼───────────────────────────┐                │
│  │         Metrics Collector                     │                │
│  │  Adaptive detail: aggregated at scale,        │                │
│  │  per-pod at small scale. Configurable.        │                │
│  └───────────────────────────────────────────────┘                │
└──────────────────────────────────────────────────────────────────┘

Validation Tiers:
  Tier 1 (above)  →  Tier 2 (KWOK/KIND)  →  Tier 3 (EKS)
  ms/run              min/run                 min/run
  10K+ runs           10s of runs             spot checks
```

## Rust Core Components

### 1. Cluster State Machine (`kubesim-core`)

The central data structure. Everything else reads/mutates this.

```rust
struct ClusterState {
    nodes: Arena<Node>,        // arena alloc for cache locality
    pods: Arena<Pod>,
    time: SimTime,             // logical ticks or wall-clock ns
    pending_queue: Vec<PodId>, // pods awaiting scheduling
    events: EventLog,          // ring buffer, configurable depth
}

struct Node {
    id: NodeId,
    instance_type: Ec2InstanceType,  // from catalog
    allocatable: Resources,          // cpu, memory, gpu, ephemeral
    allocated: Resources,
    pods: SmallVec<[PodId; 64]>,
    conditions: NodeConditions,
    labels: LabelSet,
    taints: SmallVec<[Taint; 4]>,
    cost_per_hour: f64,
    lifecycle: NodeLifecycle,        // OnDemand | Spot { interruption_prob }
}

struct Pod {
    id: PodId,
    requests: Resources,
    limits: Resources,
    phase: PodPhase,
    node: Option<NodeId>,
    scheduling_constraints: SchedulingConstraints,
    deletion_cost: Option<i32>,      // pod-deletion-cost annotation
    owner: OwnerId,                  // ReplicaSet, Job, etc.
    qos_class: QoSClass,
    priority: i32,
    topology_spread: Vec<TopologySpreadConstraint>,
}
```

### 2. Scheduler Model (`kubesim-scheduler`)

Models kube-scheduler's plugin chain:

```
Filter Phase:
  NodeResourcesFit → NodeAffinity → TaintToleration →
  PodTopologySpread → InterPodAffinity

Score Phase:
  MostAllocated | LeastAllocated → NodeAffinity →
  PodTopologySpread → InterPodAffinity → BalancedAllocation

Preemption:
  When no node passes filters, evaluate preemption candidates
  using priority, PDB, and minimizing victims.
```

Each plugin is a trait:

```rust
trait FilterPlugin {
    fn filter(&self, state: &ClusterState, pod: &Pod, node: &Node) -> FilterResult;
}

trait ScorePlugin {
    fn score(&self, state: &ClusterState, pod: &Pod, node: &Node) -> i64;
    fn weight(&self) -> i64;
}
```

Scheduler profiles are configurable — swap MostAllocated/LeastAllocated via config.

### 3. Karpenter Model (`kubesim-karpenter`)

Models Karpenter's key behaviors:

- **Provisioning**: Watches pending pods, computes NodePool/NodeClaim, selects
  cheapest instance type that fits constraints. Bin-packs multiple pending pods
  into single node launches.
- **Consolidation**: Periodically evaluates if pods on underutilized nodes can
  be rescheduled elsewhere. Models `WhenEmpty` and `WhenUnderutilized` policies.
- **Drift**: Detects nodes that no longer match NodePool spec, cordons and drains.
- **Spot interruption**: Stochastic spot interruption events based on configurable
  probability distributions per instance type.
- **Disruption budgets**: Respects NodePool disruption budgets during consolidation.

### 4. Workload Generator (`kubesim-workload`)

Four scenario generation modes:

| Mode | Source | Use Case |
|------|--------|----------|
| **Trace Replay** | Real cluster metrics (Prometheus exports, pod event logs) | Reproduce production behavior |
| **Designed** | Hand-authored YAML scenarios | Targeted experiments (your two studies) |
| **Random** | Parameterized distributions over workload types | Broad exploration |
| **Adversarial/Optimal** | Property-based + formal methods | Find worst/best case scenarios |

Workload archetypes:

```yaml
workload_types:
  web_app:
    replicas: {min: 2, max: 50}
    cpu_request: {dist: normal, mean: 250m, std: 100m}
    memory_request: {dist: normal, mean: 256Mi, std: 128Mi}
    scaling: {type: hpa, metric: cpu, target: 70%}
    churn: low          # infrequent deploys
    traffic: diurnal    # daily pattern

  ml_training:
    replicas: {fixed: 1}
    cpu_request: {dist: uniform, min: 4, max: 32}
    memory_request: {dist: uniform, min: 16Gi, max: 128Gi}
    gpu_request: {dist: choice, values: [1, 2, 4, 8]}
    duration: {dist: lognormal, mean: 4h, std: 8h}
    scaling: none
    priority: high

  batch_job:
    parallelism: {dist: uniform, min: 1, max: 100}
    cpu_request: {dist: uniform, min: 500m, max: 4}
    memory_request: {dist: uniform, min: 512Mi, max: 8Gi}
    duration: {dist: exponential, mean: 30m}
    scaling: none
    priority: low

  saas_microservice:
    replicas: {min: 3, max: 200}
    cpu_request: {dist: normal, mean: 500m, std: 200m}
    memory_request: {dist: normal, mean: 512Mi, std: 256Mi}
    scaling: {type: hpa, metric: rps, target: 1000}
    topology_spread: {max_skew: 1, topology_key: topology.kubernetes.io/zone}
    pdb: {min_available: "50%"}
    churn: medium
```

### 5. EC2 Instance Catalog (`kubesim-ec2`)

Static catalog of EC2 instance types with:
- vCPU, memory, GPU count/type, network bandwidth
- On-demand pricing per region
- Spot pricing (historical distributions or configurable)
- Instance families and generations

Source: AWS bulk pricing API / ec2instances.info data. Compiled into Rust at
build time or loaded from a JSON/Parquet catalog file.

### 6. Metrics Collector (`kubesim-metrics`)

Adaptive collection based on cluster scale:

| Cluster Size | Default Detail Level |
|-------------|---------------------|
| < 1K pods | Per-pod, per-event |
| 1K-10K pods | Per-deployment, sampled events |
| 10K-100K pods | Per-namespace, aggregated |
| 100K+ pods | Cluster-wide aggregates |

Always collected regardless of scale:
- Total cost (on-demand + spot, per time unit)
- Pod disruption count (evictions, preemptions, spot interruptions)
- Scheduling latency (pending → running time)
- Node utilization distribution (P50/P90/P99)
- Availability: fraction of time desired replicas == ready replicas

Configurable via:
```yaml
metrics:
  detail_level: auto  # auto | pod | deployment | namespace | cluster
  sample_rate: 1.0    # 0.0-1.0, fraction of events to record
  export_format: parquet  # parquet | csv | json
```

### 7. DES Event Loop (`kubesim-engine`)

```rust
enum Event {
    PodSubmitted(PodSpec),
    PodScheduled(PodId, NodeId),
    PodRunning(PodId),
    PodTerminating(PodId),
    PodDeleted(PodId),
    NodeLaunching(NodeSpec),
    NodeReady(NodeId),
    NodeCordoned(NodeId),
    NodeDrained(NodeId),
    NodeTerminated(NodeId),
    SpotInterruption(NodeId),
    HpaEvaluation(DeploymentId),
    KarpenterProvisioningLoop,
    KarpenterConsolidationLoop,
    ScaleDown(DeploymentId, u32),
    ScaleUp(DeploymentId, u32),
    MetricsSnapshot,
}
```

Two time modes:
- **Logical ticks**: Events processed in causal order, no wall-clock delays.
  Fastest — used for RL training and sweep runs.
- **Simulated wall-clock**: Events carry realistic durations (node launch: ~60-90s,
  pod startup: ~5-30s, HPA eval: 15s intervals). Used for latency studies.

## Python Layer

### PyO3 Bindings (`kubesim-py`)

```python
import kubesim

# Create simulation
sim = kubesim.Simulation(
    config="scenarios/most-vs-least-allocated.yaml",
    time_mode="logical",  # or "wall_clock"
    seed=42,
)

# Run single
result = sim.run()
print(result.total_cost, result.disruptions, result.p99_scheduling_latency)

# Batch run (parallel across cores)
results = kubesim.batch_run(
    config="scenarios/most-vs-least-allocated.yaml",
    seeds=range(10000),
    parallelism=8,
)
# returns polars DataFrame
```

### Gymnasium Environment (`kubesim-gym`)

```python
import gymnasium as gym

env = gym.make("kubesim/ClusterManagement-v0",
    cluster_size=100,
    workload_type="mixed",
)

obs, info = env.reset()
# obs: node utilizations, pending pods, cost rate, etc.

action = agent.predict(obs)
# action space: scheduling weights, consolidation thresholds, scale targets

obs, reward, terminated, truncated, info = env.step(action)
# reward = -cost - disruption_penalty + availability_bonus
```

### Property-Based Testing

```python
from hypothesis import given, strategies as st

@given(scenario=kubesim.strategies.cluster_scenario(
    max_nodes=1000,
    workload_types=["web_app", "batch_job", "ml_training"],
))
def test_no_pod_starved(scenario):
    """No pod stays Pending longer than 5 minutes (simulated)."""
    result = kubesim.run(scenario, time_mode="wall_clock")
    assert result.max_pending_duration < timedelta(minutes=5)

@given(scenario=kubesim.strategies.cluster_scenario())
def test_consolidation_reduces_cost(scenario):
    """Karpenter consolidation never increases steady-state cost."""
    baseline = kubesim.run(scenario, consolidation=False)
    consolidated = kubesim.run(scenario, consolidation=True)
    assert consolidated.steady_state_cost <= baseline.steady_state_cost * 1.01
```

### Adversarial Scenario Discovery (Mode 4)

Uses property-based testing in "find counterexample" mode combined with
coverage-guided fuzzing of the scenario space:

```python
# Find scenarios where MostAllocated is >20% more expensive than LeastAllocated
finder = kubesim.AdversarialFinder(
    objective="maximize",
    metric=lambda r: r.most_allocated.cost / r.least_allocated.cost,
    scenario_space=kubesim.ScenarioSpace(
        nodes=range(10, 1000),
        workload_mix=kubesim.workload_distributions(),
        churn_rate=uniform(0, 0.5),
    ),
    budget=10000,  # max evaluations
)
worst_cases = finder.run()
```

## Validation Pipeline (Tier 2 & 3)

```
Tier 1 result set
       │
       ▼
┌─────────────────┐     ┌──────────────────┐
│  Scenario        │────▶│  KWOK/KIND       │
│  Translator      │     │  (simkube-compat)│
│  (YAML → K8s    │     │                  │
│   manifests)     │     │  Real API server │
└─────────────────┘     │  Fake nodes      │
                        └────────┬─────────┘
                                 │
                        ┌────────▼─────────┐
                        │  Compare:         │
                        │  - Pod placement  │
                        │  - Scale timing   │
                        │  - Cost estimate  │
                        │  - Disruption cnt │
                        └────────┬─────────┘
                                 │
                        ┌────────▼─────────┐
                        │  Divergence       │
                        │  Report           │
                        │  (flag if >5%     │
                        │   delta on any    │
                        │   metric)         │
                        └──────────────────┘
```

Tier 3 (EKS) uses the same scenario translator but deploys real pods on real
nodes. Used sparingly for cost calibration and latency ground truth.

## Your Two Initial Studies

### Study 1: MostAllocated vs LeastAllocated Scheduling

```yaml
# scenarios/scheduling-comparison.yaml
study:
  name: most-vs-least-allocated
  runs: 10000
  time_mode: wall_clock

  cluster:
    node_pools:
      - instance_types: [m5.xlarge, m5.2xlarge, c5.xlarge, c5.2xlarge]
        min_nodes: 3
        max_nodes: 100
        karpenter:
          consolidation: {policy: WhenUnderutilized}

  workloads:
    - type: web_app
      count: {dist: uniform, min: 5, max: 20}
    - type: saas_microservice
      count: {dist: uniform, min: 3, max: 15}
    - type: batch_job
      count: {dist: poisson, lambda: 5}

  variants:
    - name: most_allocated
      scheduler: {scoring: MostAllocated, weight: 1}
    - name: least_allocated
      scheduler: {scoring: LeastAllocated, weight: 1}

  metrics:
    compare: [total_cost, disruption_count, p99_scheduling_latency, node_count_over_time]
```

### Study 2: Pod Deletion Cost for Node Draining During Scale-In

```yaml
# scenarios/deletion-cost-drain.yaml
study:
  name: deletion-cost-node-drain
  runs: 10000
  time_mode: wall_clock

  cluster:
    node_pools:
      - instance_types: [m5.xlarge, m5.2xlarge]
        min_nodes: 5
        max_nodes: 50
        karpenter:
          consolidation: {policy: WhenUnderutilized}

  workloads:
    - type: web_app
      count: 10
      replicas: {min: 5, max: 50}
      scaling: {type: hpa, metric: cpu, target: 70%}

  traffic_pattern:
    type: diurnal_with_spike
    peak_multiplier: 5
    duration: 24h

  variants:
    - name: baseline
      deletion_cost_strategy: none
    - name: drain_aware
      deletion_cost_strategy: prefer_emptying_nodes
      # Controller sets deletion-cost = -(pods_remaining_on_node)
      # so ReplicaSet prefers deleting pods from nearly-empty nodes
    - name: drain_aware_with_pdb
      deletion_cost_strategy: prefer_emptying_nodes
      pdb: {min_available: "80%"}

  metrics:
    compare:
      - nodes_reclaimed_per_hour
      - time_to_consolidate
      - disruption_count
      - availability_gap_seconds
      - total_cost
```

## Crate Structure

```
kubesim/
├── Cargo.toml                  # workspace
├── crates/
│   ├── kubesim-core/           # ClusterState, Node, Pod, Resources
│   ├── kubesim-engine/         # DES event loop, time modes
│   ├── kubesim-scheduler/      # kube-scheduler plugin model
│   ├── kubesim-karpenter/      # Karpenter provisioner/consolidator
│   ├── kubesim-ec2/            # EC2 instance catalog + pricing
│   ├── kubesim-workload/       # Scenario generation (all 4 modes)
│   ├── kubesim-metrics/        # Adaptive metrics collection
│   └── kubesim-py/             # PyO3 bindings
├── python/
│   └── kubesim/
│       ├── __init__.py         # Re-exports from kubesim-py
│       ├── gym_env.py          # Gymnasium environment
│       ├── strategies.py       # Hypothesis strategies
│       ├── adversarial.py      # Adversarial scenario finder
│       └── analysis.py         # Polars-based analysis helpers
├── scenarios/                  # Study definitions
├── catalogs/
│   └── ec2_instances.json      # Instance type catalog
└── validation/
    ├── kwok/                   # Tier 2 scenario translator
    └── eks/                    # Tier 3 configs
```

## Build & Run

```bash
# Build Rust core + Python bindings
cargo build --release
maturin develop --release

# Run a study
python -m kubesim run scenarios/scheduling-comparison.yaml --output results/

# RL training
python -m kubesim train --env ClusterManagement-v0 --algo PPO --timesteps 1M

# Property-based testing
pytest tests/properties/ -x --hypothesis-seed=0
```

## Implementation Phases

### Phase 1: Core Engine + Minimal Scheduler
- ClusterState, DES loop, basic Node/Pod lifecycle
- NodeResourcesFit filter + MostAllocated/LeastAllocated scoring
- EC2 instance catalog (top 20 instance types)
- Designed scenario loading (YAML)
- Basic metrics (cost, disruption count)
- PyO3 bindings: run single sim, get results

### Phase 2: Full Scheduler + Karpenter
- All scheduler filter/score plugins (affinity, topology spread, preemption)
- Karpenter provisioning + consolidation + spot interruption
- Trace replay workload mode
- Wall-clock time mode
- Adaptive metrics collection

### Phase 3: RL + Property Testing
- Gymnasium environment
- Hypothesis strategies for scenario generation
- Adversarial scenario finder (Mode 4)
- Random workload generation with archetypes

### Phase 4: Validation Pipeline
- Scenario translator (YAML → K8s manifests)
- KWOK/KIND runner + comparison harness
- EKS runner
- Divergence reporting

### Phase 5: Scale + Polish
- Arena allocator optimization for 100K+ nodes
- Parallel batch runs (rayon)
- Dashboard / visualization
- Documentation + example studies
