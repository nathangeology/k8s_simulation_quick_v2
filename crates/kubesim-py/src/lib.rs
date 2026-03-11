//! KubeSim Python — PyO3 bindings for single run and batch execution.

use pyo3::prelude::*;
use pyo3::exceptions::PyValueError;
use rayon::prelude::*;

use kubesim_core::{
    ClusterState, DeletionCostStrategy, LabelSet, Node, NodeConditions, NodeLifecycle, OwnerId, Pod, PodPhase,
    QoSClass, Resources, SchedulingConstraints, SimTime,
};
use kubesim_ec2::Catalog;
use kubesim_engine::{DeletionCostController, Engine, Event as EngineEvent, PodSpec, TimeMode};
use kubesim_metrics::{MetricsCollector, MetricsConfig as RustMetricsConfig};
use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy};
use kubesim_workload::{
    load_scenario_from_str, load_scenario_from_str_seeded, ScenarioFile, Variant,
    Event as WorkloadEvent, TimeMode as ScenarioTimeMode,
    generate_random_scenario, RandomScenarioConfig,
    RangeU32, InstanceWeight, ArchetypeWeights,
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
        created_at: SimTime(0),
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
            WorkloadEvent::SpotInterruptionCheck { time } => {
                engine.schedule(*time, EngineEvent::SpotInterruptionCheck);
            }
            _ => {}
        }
    }

    let handler = SimHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", scoring)),
        metrics: MetricsCollector::new(RustMetricsConfig::default()),
    };
    engine.add_handler(Box::new(handler));

    // Wire DeletionCostController if variant specifies a strategy
    if let Some(strategy) = variant.and_then(|v| v.deletion_cost_strategy) {
        if strategy != DeletionCostStrategy::None {
            engine.add_handler(Box::new(DeletionCostController::new(strategy)));
            engine.schedule(SimTime(0), EngineEvent::DeletionCostReconcile);
        }
    }

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

        let s = seed.unwrap_or(42);
        let (scenario, workload_events) = load_scenario_from_str_seeded(&yaml, s)
            .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;

        let tm = match time_mode {
            Some(s) => parse_time_mode(s)?,
            None => scenario_time_to_engine(scenario.study.time_mode),
        };

        Ok(Self {
            scenario,
            workload_events,
            time_mode: tm,
            seed: s,
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

    // Parse scenario once for validation and variant info; events regenerated per-seed
    let (scenario, _) = load_scenario_from_str(&yaml)
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
    // Each seed generates its own workload events from the distributions
    let results: Vec<(u64, String, SimRunResult)> = py.allow_threads(|| {
        pool.install(|| {
            work.par_iter().map(|&(seed, vi)| {
                let v = &scenario.study.variants[vi];
                let events = load_scenario_from_str_seeded(&yaml, seed)
                    .expect("scenario already validated")
                    .1;
                let r = run_single(&events, Some(v), time_mode, seed);
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

// ── Step-based simulation for Gymnasium ─────────────────────────

/// Observation returned after each step of the step-based simulation.
#[pyclass]
#[derive(Clone)]
struct StepObs {
    /// Per-node CPU utilization (allocated / allocatable), length = node_count.
    #[pyo3(get)]
    cpu_utils: Vec<f64>,
    /// Per-node memory utilization, length = node_count.
    #[pyo3(get)]
    mem_utils: Vec<f64>,
    /// Number of pending pods.
    #[pyo3(get)]
    pending_pods: u32,
    /// Total hourly cost of all nodes.
    #[pyo3(get)]
    cost_rate: f64,
    /// Fraction of non-terminal pods that are Running.
    #[pyo3(get)]
    availability: f64,
    /// Number of active nodes.
    #[pyo3(get)]
    node_count: u32,
    /// Number of active pods.
    #[pyo3(get)]
    pod_count: u32,
    /// Disruptions this step.
    #[pyo3(get)]
    disruptions: u64,
}

#[pymethods]
impl StepObs {
    fn __repr__(&self) -> String {
        format!(
            "StepObs(nodes={}, pods={}, pending={}, cost={:.3}, avail={:.3})",
            self.node_count, self.pod_count, self.pending_pods, self.cost_rate, self.availability,
        )
    }
}

/// Step-based simulation for Gymnasium integration.
///
/// Holds mutable engine + state. Each `step()` advances simulation by a time
/// window, applies the agent's action (scheduling weights), and returns an
/// observation.
#[pyclass(unsendable)]
struct StepSimulation {
    // Stored scenario for reset
    scenario: ScenarioFile,
    workload_events: Vec<WorkloadEvent>,
    time_mode: TimeMode,
    step_duration: u64,
    max_steps: u64,
    // Mutable sim state (None before first reset)
    engine: Option<Engine>,
    state: Option<ClusterState>,
    current_step: u64,
}

#[pymethods]
impl StepSimulation {
    #[new]
    #[pyo3(signature = (config, step_duration=60_000_000_000u64, max_steps=100, time_mode=None, _seed=None))]
    fn new(
        config: &str,
        step_duration: u64,
        max_steps: u64,
        time_mode: Option<&str>,
        _seed: Option<u64>,
    ) -> PyResult<Self> {
        let yaml = if Path::new(config).exists() {
            std::fs::read_to_string(config)
                .map_err(|e| PyValueError::new_err(format!("failed to read config: {e}")))?
        } else {
            config.to_string()
        };
        let s = _seed.unwrap_or(42);
        let (scenario, workload_events) = load_scenario_from_str_seeded(&yaml, s)
            .map_err(|e| PyValueError::new_err(format!("failed to parse scenario: {e}")))?;
        let tm = match time_mode {
            Some(s) => parse_time_mode(s)?,
            None => scenario_time_to_engine(scenario.study.time_mode),
        };
        Ok(Self {
            scenario,
            workload_events,
            time_mode: tm,
            step_duration,
            max_steps,
            engine: None,
            state: None,
            current_step: 0,
        })
    }

    /// Reset the simulation. Returns initial observation.
    #[pyo3(signature = (variant=None))]
    fn reset(&mut self, variant: Option<&str>) -> PyResult<StepObs> {
        let catalog = Catalog::embedded()
            .map_err(|e| PyValueError::new_err(format!("catalog: {e}")))?;

        let v = match variant {
            Some(name) => self.scenario.study.variants.iter()
                .find(|v| v.name == name),
            None => self.scenario.study.variants.first(),
        };

        let scoring = v
            .and_then(|v| v.scheduler.as_ref())
            .map(|s| scoring_from_workload(s.scoring))
            .unwrap_or(ScoringStrategy::LeastAllocated);

        let mut state = ClusterState::new();
        let mut engine = Engine::new(self.time_mode);

        for we in &self.workload_events {
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
                WorkloadEvent::SpotInterruptionCheck { time } => {
                    engine.schedule(*time, EngineEvent::SpotInterruptionCheck);
                }
                _ => {}
            }
        }

        // Add the combined handler (scheduler + metrics) to the engine
        let handler = SimHandler {
            scheduler: Scheduler::new(
                SchedulerProfile::with_scoring("default", scoring),
            ),
            metrics: MetricsCollector::new(RustMetricsConfig::default()),
        };
        engine.add_handler(Box::new(handler));

        self.engine = Some(engine);
        self.state = Some(state);
        self.current_step = 0;

        Ok(self.observe())
    }

    /// Advance simulation by one time window.
    ///
    /// `action` is `[scoring_weight, consolidation_threshold, scale_target]`:
    /// - scoring_weight: 0.0 = LeastAllocated, 1.0 = MostAllocated (blended)
    /// - consolidation_threshold: 0.0–1.0 utilization below which to consolidate
    /// - scale_target: 0.0–1.0 target utilization for scaling decisions
    ///
    /// Returns `(obs, reward, terminated, truncated, info_dict)`.
    fn step<'py>(
        &mut self,
        py: Python<'py>,
        action: Vec<f64>,
    ) -> PyResult<(StepObs, f64, bool, bool, Bound<'py, pyo3::types::PyDict>)> {
        if self.engine.is_none() {
            return Err(PyValueError::new_err("call reset() first"));
        }

        // Action values (reserved for future use with dynamic scheduler switching)
        let _scoring_weight = action.first().copied().unwrap_or(0.5);
        let _consolidation_thresh = action.get(1).copied().unwrap_or(0.5);
        let _scale_target = action.get(2).copied().unwrap_or(0.7);

        // Advance simulation by step_duration
        let step_dur = self.step_duration;
        {
            let engine = self.engine.as_mut().unwrap();
            let state = self.state.as_mut().unwrap();
            let until = SimTime(state.time.0 + step_dur);
            engine.run_until(state, until);
            state.time = until;
        }

        self.current_step += 1;
        let obs = self.observe();

        let reward = -obs.cost_rate * 0.01
            - obs.disruptions as f64 * 10.0
            + obs.availability * 5.0;

        let engine = self.engine.as_ref().unwrap();
        let state = self.state.as_ref().unwrap();
        let terminated = engine.pending() == 0 && state.pending_queue.is_empty();
        let truncated = self.current_step >= self.max_steps;

        let info = pyo3::types::PyDict::new_bound(py);
        info.set_item("step", self.current_step)?;
        info.set_item("sim_time", state.time.0)?;
        info.set_item("events_remaining", engine.pending())?;

        Ok((obs, reward, terminated, truncated, info))
    }

    /// Number of nodes in current state.
    fn node_count(&self) -> u32 {
        self.state.as_ref().map_or(0, |s| s.nodes.len())
    }

    /// Number of pods in current state.
    fn pod_count(&self) -> u32 {
        self.state.as_ref().map_or(0, |s| s.pods.len())
    }
}

impl StepSimulation {
    fn observe(&mut self) -> StepObs {
        let state = self.state.as_ref().unwrap();

        let mut cpu_utils = Vec::new();
        let mut mem_utils = Vec::new();
        let mut cost_rate = 0.0f64;
        let mut node_count = 0u32;

        for (_id, node) in state.nodes.iter() {
            node_count += 1;
            cost_rate += node.cost_per_hour;
            let cpu = if node.allocatable.cpu_millis > 0 {
                node.allocated.cpu_millis as f64 / node.allocatable.cpu_millis as f64
            } else { 0.0 };
            let mem = if node.allocatable.memory_bytes > 0 {
                node.allocated.memory_bytes as f64 / node.allocatable.memory_bytes as f64
            } else { 0.0 };
            cpu_utils.push(cpu);
            mem_utils.push(mem);
        }

        let mut running = 0u32;
        let mut active = 0u32;
        let mut pending = 0u32;
        let mut pod_count = 0u32;

        for (_id, pod) in state.pods.iter() {
            pod_count += 1;
            match pod.phase {
                PodPhase::Running => { running += 1; active += 1; }
                PodPhase::Pending => { pending += 1; active += 1; }
                PodPhase::Terminating => { active += 1; }
                _ => {}
            }
        }

        let availability = if active > 0 { running as f64 / active as f64 } else { 1.0 };

        // Disruptions are not directly accessible from the handler inside the
        // engine, so we approximate from pending count changes.
        StepObs {
            cpu_utils,
            mem_utils,
            pending_pods: pending,
            cost_rate,
            availability,
            node_count,
            pod_count,
            disruptions: 0,
        }
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Simulation>()?;
    m.add_class::<SimResult>()?;
    m.add_class::<StepSimulation>()?;
    m.add_class::<StepObs>()?;
    m.add_function(wrap_pyfunction!(batch_run, m)?)?;
    Ok(())
}
