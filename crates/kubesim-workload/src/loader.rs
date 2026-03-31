//! YAML scenario loader — parses scenario files and emits initial DES events.

use kubesim_core::{
    AffinityType, LabelSelector, LabelSet, PodAffinityTerm, Resources, SchedulingConstraints,
    SimTime, TopologySpreadConstraint, WhenUnsatisfiable, NodeAffinityTerm,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::path::Path;

use crate::events::Event;
use crate::scenario::*;

/// Errors from scenario loading.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Invalid scenario: {0}")]
    Invalid(String),
}

/// Load a scenario from a YAML file path.
pub fn load_scenario(path: &Path) -> Result<(ScenarioFile, Vec<Event>), LoadError> {
    let contents = std::fs::read_to_string(path)?;
    load_scenario_from_str(&contents)
}

/// Load a scenario from a YAML string (default seed 42).
pub fn load_scenario_from_str(yaml: &str) -> Result<(ScenarioFile, Vec<Event>), LoadError> {
    load_scenario_from_str_seeded(yaml, 42)
}

/// Load a scenario from a YAML string with an explicit RNG seed.
///
/// The seed controls how parameterized distributions (uniform, poisson, etc.)
/// in workload `count` and resource request fields are expanded into concrete
/// pod submission events.
pub fn load_scenario_from_str_seeded(yaml: &str, seed: u64) -> Result<(ScenarioFile, Vec<Event>), LoadError> {
    let mut scenario: ScenarioFile = serde_yaml::from_str(yaml)?;
    resolve_instance_type_shorthands(&mut scenario.study)
        .map_err(LoadError::Invalid)?;
    validate(&scenario.study)?;
    let mut rng = StdRng::seed_from_u64(seed);
    let events = emit_events(&scenario.study, &mut rng);
    Ok((scenario, events))
}

fn validate(study: &Study) -> Result<(), LoadError> {
    if study.name.is_empty() {
        return Err(LoadError::Invalid("study name is required".into()));
    }
    if study.cluster.node_pools.is_empty() {
        return Err(LoadError::Invalid("at least one node pool is required".into()));
    }
    if study.workloads.is_empty() {
        return Err(LoadError::Invalid("at least one workload is required".into()));
    }
    for pool in &study.cluster.node_pools {
        if pool.instance_types.is_empty() {
            return Err(LoadError::Invalid("node pool must have at least one instance type".into()));
        }
        if pool.min_nodes > pool.max_nodes {
            return Err(LoadError::Invalid("min_nodes cannot exceed max_nodes".into()));
        }
    }
    Ok(())
}

/// Emit the initial set of DES events from a parsed study.
///
/// This generates:
/// - NodeLaunching events for min_nodes in each pool
/// - PodSubmitted events for each workload's initial pods
/// - HpaEvaluation events for HPA-scaled workloads
/// - KarpenterProvisioningLoop / ConsolidationLoop if karpenter is configured
/// - TrafficChange events for traffic patterns
fn emit_events(study: &Study, rng: &mut StdRng) -> Vec<Event> {
    let mut events = Vec::new();
    let mut owner_counter: u32 = 0;

    // Bootstrap nodes from each pool
    for (pool_idx, pool) in study.cluster.node_pools.iter().enumerate() {
        for i in 0..pool.min_nodes {
            let instance_type = &pool.instance_types[i as usize % pool.instance_types.len()];
            events.push(Event::NodeLaunching {
                time: SimTime(0),
                instance_type: instance_type.clone(),
                pool_index: pool_idx as u32,
            });
        }

        // Schedule karpenter loops if configured (at t=1 so initial pods are submitted first)
        if pool.karpenter.is_some() {
            events.push(Event::KarpenterProvisioningLoop {
                time: SimTime(1),
            });
            events.push(Event::KarpenterConsolidationLoop {
                time: SimTime(1),
            });
            events.push(Event::SpotInterruptionCheck {
                time: SimTime(1),
            });
        }
    }

    // Emit initial pods for each workload
    for workload in &study.workloads {
        let count = sample_count(&workload.count, rng);

        let replicas = workload
            .replicas
            .as_ref()
            .and_then(|r| r.fixed.or(r.min))
            .unwrap_or(1);

        let priority = workload
            .priority
            .map(|p| p.to_i32())
            .unwrap_or(0);

        // When count > 1 and scale_down is defined, stagger scale-down times
        // across RS instances so they don't all scale down simultaneously.
        // Default stagger: 5 minutes per instance (configurable via scale_down_stagger).
        let stagger_ns = workload
            .scale_down_stagger
            .as_deref()
            .and_then(parse_duration_ns)
            .unwrap_or(300_000_000_000); // 5m default

        // Pacing: workload start offset
        let start_ns = workload
            .start_at
            .as_deref()
            .and_then(parse_duration_ns)
            .unwrap_or(0);

        // Pacing: interval between individual pod/RS submissions within a deployment
        let submit_interval_ns = workload
            .pod_submit_interval
            .as_deref()
            .and_then(parse_duration_ns)
            .unwrap_or(0);

        // Pacing: interval between individual pod removals during scale-down
        let sd_interval_ns = workload
            .scale_down_interval
            .as_deref()
            .and_then(parse_duration_ns)
            .unwrap_or(0);

        for i in 0..count {
            let owner_id = owner_counter;
            owner_counter += 1;

            // Stagger deployment instances by submit_interval * replicas * i
            let instance_offset = start_ns + (i as u64) * submit_interval_ns * (replicas as u64);

            let requests = sample_resources(workload, rng);

            let duration_ns = workload.duration.as_ref().and_then(|d| sample_duration(d, rng));

            // Build scheduling constraints from workload definition
            let (constraints, pod_labels) = build_scheduling_constraints(workload, owner_counter - 1);

            if replicas > 1 || workload.replicas.is_some() {
                // Workload with replicas → emit ReplicaSet at staggered time
                events.push(Event::ReplicaSetSubmitted {
                    time: SimTime(instance_offset),
                    owner_id,
                    desired_replicas: replicas,
                    requests,
                    limits: requests,
                    priority,
                    deletion_cost_strategy: DeletionCostStrategy::None,
                    deletion_cost: workload.deletion_cost,
                    scheduling_constraints: constraints,
                    labels: pod_labels,
                });
            } else {
                // Bare pod
                events.push(Event::PodSubmitted {
                    time: SimTime(instance_offset),
                    workload_name: workload.workload_type.clone(),
                    owner_id,
                    requests,
                    limits: requests,
                    priority,
                    deletion_cost: None,
                    duration_ns,
                    scheduling_constraints: constraints,
                    labels: pod_labels,
                });
            }

            // Schedule HPA if configured (15s after this workload's start)
            if let Some(ref scaling) = workload.scaling {
                if scaling.scaling_type == ScalingType::Hpa {
                    events.push(Event::HpaEvaluation {
                        time: SimTime(instance_offset + 15_000_000_000),
                        owner_id,
                    });
                }
            }

            // Emit scale-down events with per-instance stagger when count > 1
            // and per-pod interval within each scale-down batch
            if let Some(ref scale_downs) = workload.scale_down {
                let cross_instance_offset = if count > 1 { (i as u64) * stagger_ns } else { 0 };
                for sd in scale_downs {
                    if let Some(time_ns) = parse_duration_ns(&sd.at) {
                        let base_time = time_ns + cross_instance_offset;
                        if sd_interval_ns > 0 && sd.reduce_by > 1 {
                            // Emit individual scale-down events with spacing
                            for pod_idx in 0..sd.reduce_by {
                                events.push(Event::ReplicaSetScaleDown {
                                    time: SimTime(base_time + (pod_idx as u64) * sd_interval_ns),
                                    owner_id,
                                    reduce_by: 1,
                                });
                            }
                        } else {
                            events.push(Event::ReplicaSetScaleDown {
                                time: SimTime(base_time),
                                owner_id,
                                reduce_by: sd.reduce_by,
                            });
                        }
                    }
                }
            }

            // Emit scale-up events
            if let Some(ref scale_ups) = workload.scale_up {
                for su in scale_ups {
                    if let Some(time_ns) = parse_duration_ns(&su.at) {
                        if su.increase_to > replicas {
                            events.push(Event::ReplicaSetScaleUp {
                                time: SimTime(time_ns),
                                owner_id: owner_id,
                                increase_to: su.increase_to,
                            });
                        }
                    }
                }
            }

            // Emit PodResize events from resource_changes
            if let Some(ref changes) = workload.resource_changes {
                for rc in changes {
                    if let Some(time_ns) = parse_duration_ns(&rc.at) {
                        let cpu = rc.cpu_request.as_deref()
                            .and_then(parse_cpu_millis)
                            .unwrap_or(requests.cpu_millis);
                        let mem = rc.memory_request.as_deref()
                            .and_then(parse_memory_bytes)
                            .unwrap_or(requests.memory_bytes);
                        events.push(Event::PodResize {
                            time: SimTime(time_ns),
                            owner_id,
                            new_requests: Resources {
                                cpu_millis: cpu,
                                memory_bytes: mem,
                                gpu: requests.gpu,
                                ephemeral_bytes: requests.ephemeral_bytes,
                            },
                        });
                    }
                }
            }
        }
    }

    // Traffic pattern events
    if let Some(ref pattern) = study.traffic_pattern {
        emit_traffic_events(&mut events, pattern);
    }

    // Periodic metrics snapshots — use appropriate interval for time mode
    let first_snapshot = match study.time_mode {
        crate::TimeMode::Logical => SimTime(10),
        crate::TimeMode::WallClock => SimTime(60_000_000_000), // 60s
    };
    events.push(Event::MetricsSnapshot {
        time: first_snapshot,
    });

    events.sort_by_key(|e| e.time());
    events
}

/// Build SchedulingConstraints and LabelSet from a WorkloadDef.
fn build_scheduling_constraints(workload: &WorkloadDef, owner_id: u32) -> (SchedulingConstraints, LabelSet) {
    let mut constraints = SchedulingConstraints::default();
    let mut labels = LabelSet::default();

    // Set workload labels
    labels.insert("app".into(), format!("workload-{}", owner_id));
    if let Some(ref user_labels) = workload.labels {
        for (k, v) in user_labels {
            labels.insert(k.clone(), v.clone());
        }
    }

    // Topology spread constraints
    if let Some(ref tsc) = workload.topology_spread {
        constraints.topology_spread.push(TopologySpreadConstraint {
            max_skew: tsc.max_skew,
            topology_key: tsc.topology_key.clone(),
            when_unsatisfiable: WhenUnsatisfiable::DoNotSchedule,
            label_selector: LabelSelector { match_labels: labels.clone() },
        });
    }

    // Pod anti-affinity
    if let Some(ref paa) = workload.pod_anti_affinity {
        let affinity_type = if paa.affinity_type == "required" {
            AffinityType::Required
        } else {
            AffinityType::Preferred { weight: paa.weight as i32 }
        };
        let selector_key = paa.label_key.clone();
        let selector_value = paa.target_label_value.clone()
            .unwrap_or_else(|| labels.get(&selector_key).unwrap_or("").to_string());
        constraints.pod_affinity.push(PodAffinityTerm {
            affinity_type,
            label_selector: LabelSelector {
                match_labels: LabelSet(vec![(selector_key, selector_value)]),
            },
            topology_key: paa.topology_key.clone(),
            anti: true,
        });
    }

    // Node selector → node affinity required terms
    if let Some(ref ns) = workload.node_selector {
        for (k, v) in ns {
            constraints.node_affinity.push(NodeAffinityTerm {
                affinity_type: AffinityType::Required,
                match_labels: LabelSet(vec![(k.clone(), v.clone())]),
            });
        }
    }

    (constraints, labels)
}

/// Sample a concrete count from a ValueOrDist using the RNG.
fn sample_count(v: &ValueOrDist, rng: &mut StdRng) -> u32 {
    match v {
        ValueOrDist::Fixed(n) => *n,
        ValueOrDist::Dist(d) => sample_dist_u32(d, rng).max(1),
    }
}

/// Sample a u32 from a Distribution.
fn sample_dist_u32(d: &Distribution, rng: &mut StdRng) -> u32 {
    match d {
        Distribution::Uniform { min, max } => {
            let lo = min.to_f64().unwrap_or(1.0) as u32;
            let hi = max.to_f64().unwrap_or(lo as f64) as u32;
            rng.gen_range(lo..=hi)
        }
        Distribution::Poisson { lambda } => {
            // Knuth algorithm for small lambda
            let l = (-lambda).exp();
            let mut k = 0u32;
            let mut p = 1.0f64;
            loop {
                k += 1;
                p *= rng.gen::<f64>();
                if p <= l { break; }
            }
            k - 1
        }
        Distribution::Normal { mean, std } => {
            let m = mean.to_f64().unwrap_or(1.0);
            let s = std.to_f64().unwrap_or(0.0);
            box_muller_clamped(rng, m, s, 0.0, m * 4.0) as u32
        }
        _ => 1,
    }
}

/// Sample a duration in nanoseconds from a Distribution.
fn sample_duration(d: &Distribution, rng: &mut StdRng) -> Option<u64> {
    match d {
        Distribution::Uniform { min, max } => {
            let lo = min.to_duration_ns()?;
            let hi = max.to_duration_ns()?;
            Some(rng.gen_range(lo..=hi))
        }
        Distribution::Normal { mean, std } => {
            let m = mean.to_duration_ns()? as f64;
            let s = std.to_duration_ns().unwrap_or(0) as f64;
            Some(box_muller_clamped(rng, m, s, 1.0, m * 4.0) as u64)
        }
        _ => mean_duration(d),
    }
}

fn mean_duration(d: &Distribution) -> Option<u64> {
    match d {
        Distribution::Uniform { min, max } => {
            Some((min.to_duration_ns()? + max.to_duration_ns()?) / 2)
        }
        Distribution::Normal { mean, .. } => mean.to_duration_ns(),
        Distribution::Exponential { mean } => mean.to_duration_ns(),
        _ => None,
    }
}

/// Sample resource requests per-pod from distributions (or archetype defaults).
fn sample_resources(workload: &WorkloadDef, rng: &mut StdRng) -> Resources {
    let cpu = workload
        .cpu_request
        .as_ref()
        .map(|d| sample_cpu(d, rng))
        .unwrap_or_else(|| archetype_cpu(&workload.workload_type));

    let mem = workload
        .memory_request
        .as_ref()
        .map(|d| sample_mem(d, rng))
        .unwrap_or_else(|| archetype_mem(&workload.workload_type));

    let gpu = workload
        .gpu_request
        .as_ref()
        .map(|d| sample_gpu(d, rng))
        .unwrap_or(0);

    Resources {
        cpu_millis: cpu,
        memory_bytes: mem,
        gpu,
        ephemeral_bytes: 0,
    }
}

fn sample_cpu(d: &Distribution, rng: &mut StdRng) -> u64 {
    match d {
        Distribution::Uniform { min, max } => {
            let lo = min.to_cpu_millis().unwrap_or(100);
            let hi = max.to_cpu_millis().unwrap_or(lo);
            rng.gen_range(lo..=hi)
        }
        Distribution::Normal { mean, std } => {
            let m = mean.to_cpu_millis().unwrap_or(500) as f64;
            let s = std.to_cpu_millis().unwrap_or(0) as f64;
            box_muller_clamped(rng, m, s, 1.0, m * 4.0) as u64
        }
        _ => dist_mean_cpu(d).unwrap_or(500),
    }
}

fn sample_mem(d: &Distribution, rng: &mut StdRng) -> u64 {
    match d {
        Distribution::Uniform { min, max } => {
            let lo = min.to_memory_bytes().unwrap_or(64 * 1024 * 1024);
            let hi = max.to_memory_bytes().unwrap_or(lo);
            rng.gen_range(lo..=hi)
        }
        Distribution::Normal { mean, std } => {
            let m = mean.to_memory_bytes().unwrap_or(512 * 1024 * 1024) as f64;
            let s = std.to_memory_bytes().unwrap_or(0) as f64;
            box_muller_clamped(rng, m, s, 1.0, m * 4.0) as u64
        }
        _ => dist_mean_mem(d).unwrap_or(512 * 1024 * 1024),
    }
}

fn sample_gpu(d: &Distribution, rng: &mut StdRng) -> u32 {
    match d {
        Distribution::Uniform { min, max } => {
            let lo = min.to_f64().unwrap_or(0.0) as u32;
            let hi = max.to_f64().unwrap_or(lo as f64) as u32;
            rng.gen_range(lo..=hi)
        }
        Distribution::Choice { values } if !values.is_empty() => {
            let idx = rng.gen_range(0..values.len());
            values[idx].to_f64().unwrap_or(0.0) as u32
        }
        _ => dist_mean_gpu(d).unwrap_or(0),
    }
}

/// Box-Muller normal sample clamped to [lo, hi].
fn box_muller_clamped(rng: &mut StdRng, mean: f64, std: f64, lo: f64, hi: f64) -> f64 {
    let u1: f64 = rng.gen::<f64>().max(1e-10);
    let u2: f64 = rng.gen();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    (mean + std * z).clamp(lo, hi)
}

fn dist_mean_cpu(d: &Distribution) -> Option<u64> {
    match d {
        Distribution::Normal { mean, .. } | Distribution::Lognormal { mean, .. } => mean.to_cpu_millis(),
        Distribution::Uniform { min, max } => {
            let lo = min.to_cpu_millis()?;
            let hi = max.to_cpu_millis()?;
            Some((lo + hi) / 2)
        }
        Distribution::Exponential { mean } => mean.to_cpu_millis(),
        Distribution::Choice { values } if !values.is_empty() => {
            values.first().and_then(|v| v.to_cpu_millis())
        }
        _ => None,
    }
}

fn dist_mean_mem(d: &Distribution) -> Option<u64> {
    match d {
        Distribution::Normal { mean, .. } | Distribution::Lognormal { mean, .. } => mean.to_memory_bytes(),
        Distribution::Uniform { min, max } => {
            let lo = min.to_memory_bytes()?;
            let hi = max.to_memory_bytes()?;
            Some((lo + hi) / 2)
        }
        Distribution::Exponential { mean } => mean.to_memory_bytes(),
        Distribution::Choice { values } if !values.is_empty() => {
            values.first().and_then(|v| v.to_memory_bytes())
        }
        _ => None,
    }
}

fn dist_mean_gpu(d: &Distribution) -> Option<u32> {
    match d {
        Distribution::Choice { values } if !values.is_empty() => {
            values.first().and_then(|v| v.to_f64()).map(|v| v as u32)
        }
        Distribution::Uniform { min, max } => {
            let lo = min.to_f64()?;
            let hi = max.to_f64()?;
            Some(((lo + hi) / 2.0) as u32)
        }
        _ => None,
    }
}

/// Default CPU (millicores) for known workload archetypes.
fn archetype_cpu(workload_type: &str) -> u64 {
    match workload_type {
        "web_app" => 250,
        "ml_training" => 8000,
        "batch_job" => 1000,
        "saas_microservice" => 500,
        _ => 500,
    }
}

/// Default memory (bytes) for known workload archetypes.
fn archetype_mem(workload_type: &str) -> u64 {
    match workload_type {
        "web_app" => 256 * 1024 * 1024,
        "ml_training" => 32 * 1024 * 1024 * 1024,
        "batch_job" => 2 * 1024 * 1024 * 1024,
        "saas_microservice" => 512 * 1024 * 1024,
        _ => 512 * 1024 * 1024,
    }
}

/// Emit traffic change events for a traffic pattern.
fn emit_traffic_events(events: &mut Vec<Event>, pattern: &TrafficPattern) {
    let duration_ns = pattern
        .duration
        .as_deref()
        .and_then(|s| {
            if let Some(v) = s.strip_suffix('h') {
                v.parse::<f64>().ok().map(|v| (v * 3_600_000_000_000.0) as u64)
            } else {
                None
            }
        })
        .unwrap_or(86_400_000_000_000); // default 24h

    let peak = pattern.peak_multiplier.unwrap_or(2.0);
    let steps = 24u64; // hourly granularity
    let step_ns = duration_ns / steps;

    for i in 0..steps {
        // Simple sinusoidal traffic model: peak at midpoint
        let phase = std::f64::consts::PI * 2.0 * (i as f64) / (steps as f64);
        let multiplier = 1.0 + (peak - 1.0) * (0.5 * (1.0 - phase.cos()));
        events.push(Event::TrafficChange {
            time: SimTime(i * step_ns),
            multiplier,
        });
    }
}

/// Generate per-variant configuration events.
///
/// Call this once per variant before running the simulation for that variant.
pub fn variant_events(variant: &Variant) -> Vec<Event> {
    let mut events = Vec::new();
    if let Some(ref sched) = variant.scheduler {
        events.push(Event::ConfigureScheduler {
            scoring: sched.scoring,
            weight: sched.weight,
        });
    }
    if let Some(strategy) = variant.deletion_cost_strategy {
        events.push(Event::ConfigureDeletionCost { strategy });
    }
    events
}
