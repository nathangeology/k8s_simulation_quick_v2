//! Random workload generation with archetypes.
//!
//! Generates realistic cluster scenarios from parameterized distributions over
//! workload archetypes (web_app, ml_training, batch_job, saas_microservice).
//! Seeded RNG for reproducibility.

use kubesim_core::{Resources, SimTime};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::events::Event;

/// Configuration for random scenario generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomScenarioConfig {
    /// RNG seed for reproducibility.
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Number of workloads to generate.
    #[serde(default = "default_workload_count")]
    pub workload_count: RangeU32,
    /// Cluster size range (total nodes).
    #[serde(default = "default_cluster_size")]
    pub cluster_size: RangeU32,
    /// Instance type mix with relative weights.
    #[serde(default = "default_instance_mix")]
    pub instance_mix: Vec<InstanceWeight>,
    /// Archetype weights (relative probability of each type).
    #[serde(default = "default_archetype_weights")]
    pub archetype_weights: ArchetypeWeights,
}

fn default_seed() -> u64 { 42 }
fn default_workload_count() -> RangeU32 { RangeU32 { min: 10, max: 50 } }
fn default_cluster_size() -> RangeU32 { RangeU32 { min: 5, max: 100 } }

fn default_instance_mix() -> Vec<InstanceWeight> {
    vec![
        InstanceWeight { instance_type: "m5.xlarge".into(), weight: 4 },
        InstanceWeight { instance_type: "m5.2xlarge".into(), weight: 3 },
        InstanceWeight { instance_type: "c5.xlarge".into(), weight: 2 },
        InstanceWeight { instance_type: "c5.2xlarge".into(), weight: 1 },
    ]
}

fn default_archetype_weights() -> ArchetypeWeights {
    ArchetypeWeights { web_app: 4, ml_training: 1, batch_job: 2, saas_microservice: 3 }
}

impl Default for RandomScenarioConfig {
    fn default() -> Self {
        Self {
            seed: default_seed(),
            workload_count: default_workload_count(),
            cluster_size: default_cluster_size(),
            instance_mix: default_instance_mix(),
            archetype_weights: default_archetype_weights(),
        }
    }
}

/// Inclusive u32 range.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RangeU32 {
    pub min: u32,
    pub max: u32,
}

/// Weighted instance type entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceWeight {
    pub instance_type: String,
    pub weight: u32,
}

/// Relative weights for each workload archetype.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeWeights {
    pub web_app: u32,
    pub ml_training: u32,
    pub batch_job: u32,
    pub saas_microservice: u32,
}

/// Which archetype was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Archetype {
    WebApp,
    MlTraining,
    BatchJob,
    SaasMicroservice,
}

/// Generate a random scenario, returning DES events.
pub fn generate_random_scenario(config: &RandomScenarioConfig) -> Vec<Event> {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut events = Vec::new();

    // Generate nodes
    let node_count = rng.gen_range(config.cluster_size.min..=config.cluster_size.max);
    let total_inst_weight: u32 = config.instance_mix.iter().map(|i| i.weight).sum();

    for _ in 0..node_count {
        let instance_type = if total_inst_weight > 0 {
            weighted_pick(&config.instance_mix, &mut rng, total_inst_weight)
        } else {
            "m5.xlarge"
        };
        events.push(Event::NodeLaunching {
            time: SimTime(0),
            instance_type: instance_type.to_string(),
            pool_index: 0,
        });
    }

    // Generate workloads
    let wl_count = rng.gen_range(config.workload_count.min..=config.workload_count.max);
    let aw = &config.archetype_weights;
    let total_aw = aw.web_app + aw.ml_training + aw.batch_job + aw.saas_microservice;

    let mut owner_counter: u32 = 0;
    // Spread workload submissions over the first 10 minutes
    let spread_ns: u64 = 600_000_000_000; // 10 minutes in ns

    for wl_idx in 0..wl_count {
        let archetype = pick_archetype(aw, total_aw, &mut rng);
        let owner_id = owner_counter;
        owner_counter += 1;

        // Stagger workload start times across the spread window
        let start_ns = if wl_count > 1 {
            (wl_idx as u64) * spread_ns / (wl_count as u64 - 1).max(1)
        } else {
            0
        };

        match archetype {
            Archetype::WebApp => emit_web_app(&mut rng, &mut events, owner_id, start_ns),
            Archetype::MlTraining => emit_ml_training(&mut rng, &mut events, owner_id, start_ns),
            Archetype::BatchJob => emit_batch_job(&mut rng, &mut events, owner_id, start_ns),
            Archetype::SaasMicroservice => emit_saas_microservice(&mut rng, &mut events, owner_id, start_ns),
        }
    }

    // Karpenter loops
    events.push(Event::KarpenterProvisioningLoop { time: SimTime(0) });
    events.push(Event::KarpenterConsolidationLoop { time: SimTime(0) });
    events.push(Event::SpotInterruptionCheck { time: SimTime(0) });

    // Metrics snapshot
    events.push(Event::MetricsSnapshot { time: SimTime(60_000_000_000) });

    events.sort_by_key(|e| e.time());
    events
}

fn weighted_pick<'a>(mix: &'a [InstanceWeight], rng: &mut StdRng, total: u32) -> &'a str {
    let mut r = rng.gen_range(0..total);
    for iw in mix {
        if r < iw.weight {
            return &iw.instance_type;
        }
        r -= iw.weight;
    }
    &mix[0].instance_type
}

fn pick_archetype(aw: &ArchetypeWeights, total: u32, rng: &mut StdRng) -> Archetype {
    if total == 0 {
        return Archetype::WebApp;
    }
    let mut r = rng.gen_range(0..total);
    if r < aw.web_app { return Archetype::WebApp; }
    r -= aw.web_app;
    if r < aw.ml_training { return Archetype::MlTraining; }
    r -= aw.ml_training;
    if r < aw.batch_job { return Archetype::BatchJob; }
    Archetype::SaasMicroservice
}

/// Normal-ish sample clamped to [lo, hi], using Box-Muller on uniform RNG.
fn normal_clamped(rng: &mut StdRng, mean: f64, std: f64, lo: f64, hi: f64) -> f64 {
    let u1: f64 = rng.gen::<f64>().max(1e-10);
    let u2: f64 = rng.gen();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    (mean + std * z).clamp(lo, hi)
}

// ── Archetype emitters ──────────────────────────────────────────

fn emit_web_app(rng: &mut StdRng, events: &mut Vec<Event>, owner_id: u32, start_ns: u64) {
    let replicas = rng.gen_range(2..=20);
    let cpu = normal_clamped(rng, 250.0, 100.0, 50.0, 1000.0) as u64;
    let mem = normal_clamped(rng, 256.0, 128.0, 64.0, 1024.0) as u64 * 1024 * 1024;
    let pod_interval: u64 = 1_000_000_000; // 1s between pods (rolling deploy)

    for r in 0..replicas {
        events.push(Event::PodSubmitted {
            time: SimTime(start_ns + (r as u64) * pod_interval),
            workload_name: "web_app".into(),
            owner_id,
            requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            priority: 0,
            deletion_cost: None,
            duration_ns: None,
        });
    }
    // HPA 15s after last pod
    events.push(Event::HpaEvaluation {
        time: SimTime(start_ns + (replicas as u64) * pod_interval + 15_000_000_000),
        owner_id,
    });
}

fn emit_ml_training(rng: &mut StdRng, events: &mut Vec<Event>, owner_id: u32, start_ns: u64) {
    let cpu = rng.gen_range(4000..=32000);
    let mem = rng.gen_range(16..=128) as u64 * 1024 * 1024 * 1024;
    let gpu_choices = [1, 2, 4, 8];
    let gpu = gpu_choices[rng.gen_range(0..gpu_choices.len())];

    events.push(Event::PodSubmitted {
        time: SimTime(start_ns),
        workload_name: "ml_training".into(),
        owner_id,
        requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu, ephemeral_bytes: 0 },
        limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu, ephemeral_bytes: 0 },
        priority: 100,
        deletion_cost: None,
        duration_ns: None,
    });
}

fn emit_batch_job(rng: &mut StdRng, events: &mut Vec<Event>, owner_id: u32, start_ns: u64) {
    let parallelism = rng.gen_range(1..=20);
    let cpu = rng.gen_range(500..=4000);
    let mem = rng.gen_range(512..=8192) as u64 * 1024 * 1024;
    let pod_interval: u64 = 500_000_000; // 0.5s between parallel pods

    for p in 0..parallelism {
        events.push(Event::PodSubmitted {
            time: SimTime(start_ns + (p as u64) * pod_interval),
            workload_name: "batch_job".into(),
            owner_id,
            requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            priority: -100,
            deletion_cost: None,
            duration_ns: None,
        });
    }
}

fn emit_saas_microservice(rng: &mut StdRng, events: &mut Vec<Event>, owner_id: u32, start_ns: u64) {
    let replicas = rng.gen_range(3..=30);
    let cpu = normal_clamped(rng, 500.0, 200.0, 100.0, 2000.0) as u64;
    let mem = normal_clamped(rng, 512.0, 256.0, 128.0, 2048.0) as u64 * 1024 * 1024;
    let pod_interval: u64 = 1_000_000_000; // 1s between pods

    for r in 0..replicas {
        events.push(Event::PodSubmitted {
            time: SimTime(start_ns + (r as u64) * pod_interval),
            workload_name: "saas_microservice".into(),
            owner_id,
            requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            priority: 0,
            deletion_cost: None,
            duration_ns: None,
        });
    }
    // HPA 15s after last pod
    events.push(Event::HpaEvaluation {
        time: SimTime(start_ns + (replicas as u64) * pod_interval + 15_000_000_000),
        owner_id,
    });
}
