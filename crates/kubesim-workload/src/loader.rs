//! YAML scenario loader — parses scenario files and emits initial DES events.

use kubesim_core::{Resources, SimTime};
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

/// Load a scenario from a YAML string.
pub fn load_scenario_from_str(yaml: &str) -> Result<(ScenarioFile, Vec<Event>), LoadError> {
    let scenario: ScenarioFile = serde_yaml::from_str(yaml)?;
    validate(&scenario.study)?;
    let events = emit_events(&scenario.study);
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
fn emit_events(study: &Study) -> Vec<Event> {
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

        // Schedule karpenter loops if configured
        if pool.karpenter.is_some() {
            events.push(Event::KarpenterProvisioningLoop {
                time: SimTime(0),
            });
            events.push(Event::KarpenterConsolidationLoop {
                time: SimTime(0),
            });
        }
    }

    // Emit initial pods for each workload
    for workload in &study.workloads {
        let count = match &workload.count {
            ValueOrDist::Fixed(n) => *n,
            ValueOrDist::Dist(_) => 1, // deterministic fallback; RNG-based expansion at runtime
        };

        let replicas = workload
            .replicas
            .as_ref()
            .and_then(|r| r.fixed.or(r.min))
            .unwrap_or(1);

        let priority = workload
            .priority
            .map(|p| p.to_i32())
            .unwrap_or(0);

        let requests = resolve_resources(workload);

        for _ in 0..count {
            let owner_id = owner_counter;
            owner_counter += 1;

            for _ in 0..replicas {
                events.push(Event::PodSubmitted {
                    time: SimTime(0),
                    workload_name: workload.workload_type.clone(),
                    owner_id,
                    requests,
                    limits: requests, // limits = requests for Guaranteed QoS default
                    priority,
                    deletion_cost: None,
                });
            }

            // Schedule HPA if configured
            if let Some(ref scaling) = workload.scaling {
                if scaling.scaling_type == ScalingType::Hpa {
                    events.push(Event::HpaEvaluation {
                        time: SimTime(15_000_000_000), // 15s initial delay
                        owner_id,
                    });
                }
            }
        }
    }

    // Traffic pattern events
    if let Some(ref pattern) = study.traffic_pattern {
        emit_traffic_events(&mut events, pattern);
    }

    // Periodic metrics snapshots
    events.push(Event::MetricsSnapshot {
        time: SimTime(60_000_000_000), // first snapshot at 60s
    });

    events.sort_by_key(|e| e.time());
    events
}

/// Resolve default resource requests from workload archetype or explicit fields.
fn resolve_resources(workload: &WorkloadDef) -> Resources {
    let cpu = workload
        .cpu_request
        .as_ref()
        .and_then(|d| dist_mean_cpu(d))
        .unwrap_or_else(|| archetype_cpu(&workload.workload_type));

    let mem = workload
        .memory_request
        .as_ref()
        .and_then(|d| dist_mean_mem(d))
        .unwrap_or_else(|| archetype_mem(&workload.workload_type));

    let gpu = workload
        .gpu_request
        .as_ref()
        .and_then(|d| dist_mean_gpu(d))
        .unwrap_or(0);

    Resources {
        cpu_millis: cpu,
        memory_bytes: mem,
        gpu,
        ephemeral_bytes: 0,
    }
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
