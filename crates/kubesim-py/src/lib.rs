//! KubeSim Python — PyO3 bindings for single run and batch execution.

use pyo3::prelude::*;
use pyo3::exceptions::PyValueError;
use rayon::prelude::*;

use kubesim_core::{
    ClusterState, LabelSet, Node, NodeConditions, NodeLifecycle, OwnerId, Pod, PodPhase,
    QoSClass, Resources, SchedulingConstraints, SimTime,
};
use kubesim_ec2::Catalog;
use kubesim_engine::{Engine, Event as EngineEvent, PodSpec, TimeMode};
use kubesim_metrics::{MetricsCollector, MetricsConfig as RustMetricsConfig};
use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy};
use kubesim_workload::{
    load_scenario_from_str, ScenarioFile, Variant,
    Event as WorkloadEvent, TimeMode as ScenarioTimeMode,
};

use std::path::Path;

// ── Helpers ─────────────────────────────────────────────────────

fn scenario_time_to_engine(t: ScenarioTimeMode) -> TimeMode {
    match t {
        ScenarioTimeMode::Logical => TimeMode::Logical,
        ScenarioTimeMode::WallClock => TimeMode::WallClock,
    }
}

fn parse_time_mode(s: &str) -> PyResult<TimeMode> {
    match s {
        "logical" => Ok(TimeMode::Logical),
        "wall_clock" | "wallclock" => Ok(TimeMode::WallClock),
        _ => Err(PyValueError::new_err(format!(
            "invalid time_mode: {s:?}, expected 'logical' or 'wall_clock'"
        ))),
    }
}

fn scoring_from_workload(s: kubesim_workload::ScoringStrategy) -> ScoringStrategy {
    match s {
        kubesim_workload::ScoringStrategy::MostAllocated => ScoringStrategy::MostAllocated,
        kubesim_workload::ScoringStrategy::LeastAllocated => ScoringStrategy::LeastAllocated,
    }
}

fn instance_to_node(catalog: &Catalog, instance_type: &str) -> Node {
    let (cpu, mem, gpu, cost) = catalog
        .get(instance_type)
        .map(|it| (
            it.vcpu as u64 * 1000,
            it.memory_gib as u64 * 1024 * 1024 * 1024,
            it.gpu_count,
            it.on_demand_price_per_hour,
        ))
        .unwrap_or((4000, 16 * 1024 * 1024 * 1024, 0, 0.192));

    Node {
        instance_type: instance_type.to_string(),
        allocatable: Resources { cpu_millis: cpu, memory_bytes: mem, gpu, ephemeral_bytes: 0 },
        allocated: Resources::default(),
        pods: Default::default(),
        conditions: NodeConditions { ready: true, ..Default::default() },
        labels: LabelSet::default(),
        taints: Default::default(),
        cost_per_hour: cost,
        lifecycle: NodeLifecycle::OnDemand,
        cordoned: false,
    }
}

// ── Combined event handler ──────────────────────────────────────

struct SimHandler {
    scheduler: Scheduler,
    metrics: MetricsCollector,
}

impl kubesim_engine::EventHandler for SimHandler {
    fn handle(
        &mut self,
        event: &EngineEvent,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<kubesim_engine::ScheduledEvent> {
        // Forward to metrics collector
        let _ = self.metrics.handle(event, time, state);

        match event {
            EngineEvent::PodSubmitted(spec) => {
                let pod = Pod {
                    requests: spec.requests,
                    limits: spec.limits,
                    phase: PodPhase::Pending,
                    node: None,
                    scheduling_constraints: spec.scheduling_constraints.clone(),
                    deletion_cost: None,
                    owner: spec.owner,
                    qos_class: QoSClass::Burstable,
                    priority: spec.priority,
                    labels: spec.labels.clone(),
                };
                let pod_id = state.submit_pod(pod);

                if let kubesim_scheduler::ScheduleResult::Bound(node_id) =
                    self.scheduler.schedule_one(state, pod_id)
                {
                    state.bind_pod(pod_id, node_id);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

// ── Single simulation run ───────────────────────────────────────

struct SimRunResult {
    events_processed: u64,
    total_cost_per_hour: f64,
    node_count: u32,
    pod_count: u32,
    running_pods: u32,
    pending_pods: u32,
    final_time: u64,
}

fn run_single(
    workload_events: &[WorkloadEvent],
    variant: Option<&Variant>,
    time_mode: TimeMode,
    _seed: u64,
) -> SimRunResult {
    let catalog = Catalog::embedded().expect("embedded EC2 catalog");

    let mut state = ClusterState::new();
    let mut engine = Engine::new(time_mode);

    let scoring = variant
        .and_then(|v| v.scheduler.as_ref())
        .map(|s| scoring_from_workload(s.scoring))
        .unwrap_or(ScoringStrategy::LeastAllocated);

    // Seed engine from workload events
    for we in workload_events {
        match we {
            WorkloadEvent::NodeLaunching { instance_type, .. } => {
                let node = instance_to_node(&catalog, instance_type);
                state.add_node(node);
            }
            WorkloadEvent::PodSubmitted { time, requests, limits, priority, owner_id, .. } => {
                engine.schedule(*time, EngineEvent::PodSubmitted(PodSpec {
                    requests: *requests,
                    limits: *limits,
                    owner: OwnerId(*owner_id),
                    priority: *priority,
                    labels: LabelSet::default(),
                    scheduling_constraints: SchedulingConstraints::default(),
                }));
            }
            WorkloadEvent::MetricsSnapshot { time } => {
                engine.schedule(*time, EngineEvent::MetricsSnapshot);
            }
            WorkloadEvent::HpaEvaluation { time, owner_id } => {
                engine.schedule(
                    *time,
                    EngineEvent::HpaEvaluation(kubesim_engine::DeploymentId(*owner_id)),
                );
            }
            WorkloadEvent::KarpenterProvisioningLoop { time } => {
                engine.schedule(*time, EngineEvent::KarpenterProvisioningLoop);
            }
            WorkloadEvent::KarpenterConsolidationLoop { time } => {
                engine.schedule(*time, EngineEvent::KarpenterConsolidationLoop);
            }
            _ => {}
        }
    }

    let handler = SimHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", scoring)),
        metrics: MetricsCollector::new(RustMetricsConfig::default()),
    };
    engine.add_handler(Box::new(handler));

    let events_processed = engine.run_to_completion(&mut state);

    // Collect final state summary
    let mut total_cost = 0.0f64;
    let mut node_count = 0u32;
    for (_id, node) in state.nodes.iter() {
        total_cost += node.cost_per_hour;
        node_count += 1;
    }

    let mut running = 0u32;
    let mut pending = 0u32;
    let mut pod_count = 0u32;
    for (_id, pod) in state.pods.iter() {
        pod_count += 1;
        match pod.phase {
            PodPhase::Running => running += 1,
            PodPhase::Pending => pending += 1,
            _ => {}
        }
    }

    SimRunResult {
        events_processed,
        total_cost_per_hour: total_cost,
        node_count,
        pod_count,
        running_pods: running,
        pending_pods: pending,
        final_time: state.time.0,
    }
}

// ── Python types ────────────────────────────────────────────────

#[pyclass]
#[derive(Clone)]
struct SimResult {
    #[pyo3(get)]
    events_processed: u64,
    #[pyo3(get)]
    total_cost_per_hour: f64,
    #[pyo3(get)]
    node_count: u32,
    #[pyo3(get)]
    pod_count: u32,
    #[pyo3(get)]
    running_pods: u32,
    #[pyo3(get)]
    pending_pods: u32,
    #[pyo3(get)]
    final_time: u64,
    #[pyo3(get)]
    variant: String,
    #[pyo3(get)]
    seed: u64,
}

#[pymethods]
impl SimResult {
    fn __repr__(&self) -> String {
        format!(
            "SimResult(variant={:?}, cost={:.4}, nodes={}, pods={}/{} running, events={})",
            self.variant, self.total_cost_per_hour, self.node_count,
            self.running_pods, self.pod_count, self.events_processed,
        )
    }

    /// Export as dict for polars/pandas DataFrame construction.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("seed", self.seed)?;
        dict.set_item("variant", &self.variant)?;
        dict.set_item("events_processed", self.events_processed)?;
        dict.set_item("total_cost_per_hour", self.total_cost_per_hour)?;
        dict.set_item("node_count", self.node_count)?;
        dict.set_item("pod_count", self.pod_count)?;
        dict.set_item("running_pods", self.running_pods)?;
        dict.set_item("pending_pods", self.pending_pods)?;
        dict.set_item("final_time", self.final_time)?;
        Ok(dict)
    }
}

#[pyclass]
struct Simulation {
    scenario: ScenarioFile,
    workload_events: Vec<WorkloadEvent>,
    time_mode: TimeMode,
    seed: u64,
}

#[pymethods]
impl Simulation {
    #[new]
    #[pyo3(signature = (config, time_mode=None, seed=None))]
    fn new(config: &str, time_mode: Option<&str>, seed: Option<u64>) -> PyResult<Self> {
        let yaml = if Path::new(config).exists() {
            std::fs::read_to_string(config)
                .map_err(|e| PyValueError::new_err(format!("failed to read config: {e}")))?
        } else {
            config.to_string()
        };

        let (scenario, workload_events) = load_scenario_from_str(&yaml)
            .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;

        let tm = match time_mode {
            Some(s) => parse_time_mode(s)?,
            None => scenario_time_to_engine(scenario.study.time_mode),
        };

        Ok(Self {
            scenario,
            workload_events,
            time_mode: tm,
            seed: seed.unwrap_or(42),
        })
    }

    /// Run simulation for a single variant.
    #[pyo3(signature = (variant=None))]
    fn run(&self, variant: Option<&str>) -> PyResult<SimResult> {
        let v = match variant {
            Some(name) => self.scenario.study.variants.iter()
                .find(|v| v.name == name)
                .ok_or_else(|| PyValueError::new_err(format!("variant {name:?} not found")))?,
            None => self.scenario.study.variants.first()
                .ok_or_else(|| PyValueError::new_err("no variants defined in scenario"))?,
        };

        let r = run_single(&self.workload_events, Some(v), self.time_mode, self.seed);

        Ok(SimResult {
            events_processed: r.events_processed,
            total_cost_per_hour: r.total_cost_per_hour,
            node_count: r.node_count,
            pod_count: r.pod_count,
            running_pods: r.running_pods,
            pending_pods: r.pending_pods,
            final_time: r.final_time,
            variant: v.name.clone(),
            seed: self.seed,
        })
    }

    /// Run all variants, returning a list of SimResult.
    fn run_all(&self) -> PyResult<Vec<SimResult>> {
        if self.scenario.study.variants.is_empty() {
            return Err(PyValueError::new_err("no variants defined in scenario"));
        }

        Ok(self.scenario.study.variants.iter().map(|v| {
            let r = run_single(&self.workload_events, Some(v), self.time_mode, self.seed);
            SimResult {
                events_processed: r.events_processed,
                total_cost_per_hour: r.total_cost_per_hour,
                node_count: r.node_count,
                pod_count: r.pod_count,
                running_pods: r.running_pods,
                pending_pods: r.pending_pods,
                final_time: r.final_time,
                variant: v.name.clone(),
                seed: self.seed,
            }
        }).collect())
    }
}

/// Batch run a scenario across multiple seeds with rayon parallelism.
///
/// Returns a list of dicts suitable for `polars.DataFrame(batch_run(...))`.
#[pyfunction]
#[pyo3(signature = (config, seeds, parallelism=None))]
fn batch_run<'py>(
    py: Python<'py>,
    config: &str,
    seeds: Vec<u64>,
    parallelism: Option<usize>,
) -> PyResult<Bound<'py, pyo3::types::PyList>> {
    let yaml = if Path::new(config).exists() {
        std::fs::read_to_string(config)
            .map_err(|e| PyValueError::new_err(format!("failed to read config: {e}")))?
    } else {
        config.to_string()
    };

    let (scenario, workload_events) = load_scenario_from_str(&yaml)
        .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;

    let time_mode = scenario_time_to_engine(scenario.study.time_mode);

    if scenario.study.variants.is_empty() {
        return Err(PyValueError::new_err("no variants defined in scenario"));
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallelism.unwrap_or(0))
        .build()
        .map_err(|e| PyValueError::new_err(format!("failed to create thread pool: {e}")))?;

    // Build work items: (seed, variant_index)
    let work: Vec<(u64, usize)> = seeds.iter()
        .flat_map(|&s| (0..scenario.study.variants.len()).map(move |vi| (s, vi)))
        .collect();

    // Run in parallel, releasing the GIL
    let results: Vec<(u64, String, SimRunResult)> = py.allow_threads(|| {
        pool.install(|| {
            work.par_iter().map(|&(seed, vi)| {
                let v = &scenario.study.variants[vi];
                let r = run_single(&workload_events, Some(v), time_mode, seed);
                (seed, v.name.clone(), r)
            }).collect()
        })
    });

    // Convert to list of dicts
    let list = pyo3::types::PyList::empty_bound(py);
    for (seed, variant_name, r) in &results {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("seed", seed)?;
        dict.set_item("variant", variant_name)?;
        dict.set_item("events_processed", r.events_processed)?;
        dict.set_item("total_cost_per_hour", r.total_cost_per_hour)?;
        dict.set_item("node_count", r.node_count)?;
        dict.set_item("pod_count", r.pod_count)?;
        dict.set_item("running_pods", r.running_pods)?;
        dict.set_item("pending_pods", r.pending_pods)?;
        dict.set_item("final_time", r.final_time)?;
        list.append(dict)?;
    }

    Ok(list)
}

#[pymodule]
fn kubesim_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Simulation>()?;
    m.add_class::<SimResult>()?;
    m.add_function(wrap_pyfunction!(batch_run, m)?)?;
    Ok(())
}
