//! Scale performance benchmark for KubeSim.
//!
//! Measures wall-clock time, events processed, and RSS memory at increasing
//! cluster scales and simulation durations. Outputs results to
//! results/scale-perf-benchmark.txt.

use kubesim_core::{
    ClusterState, LabelSet, Node, NodeConditions, NodeLifecycle, OwnerId, Pod, PodPhase,
    QoSClass, Resources, SchedulingConstraints, SimTime,
};
use kubesim_ec2::Catalog;
use kubesim_engine::{Engine, Event as EngineEvent, EventHandler, PodSpec, ScheduledEvent, TimeMode};
use kubesim_metrics::{MetricsCollector, MetricsConfig};
use kubesim_scheduler::{Scheduler, SchedulerProfile, ScoringStrategy};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use std::fmt::Write as _;
use std::fs;
use std::time::Instant;

const WALL_CLOCK_LIMIT_SECS: f64 = 60.0;

// ── RSS measurement ─────────────────────────────────────────────

fn rss_bytes() -> u64 {
    // Use `ps` to read RSS of current process (works on macOS and Linux)
    let pid = std::process::id();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024) // ps reports in KiB
        .unwrap_or(0)
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0))
    }
}

// ── Sim handler ─────────────────────────────────────────────────

struct SimHandler {
    scheduler: Scheduler,
    metrics: MetricsCollector,
}

impl EventHandler for SimHandler {
    fn handle(
        &mut self,
        event: &EngineEvent,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
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

// ── Benchmark helpers ───────────────────────────────────────────

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

struct ScaleResult {
    nodes: u32,
    pods: u32,
    events: u64,
    wall_secs: f64,
    rss: u64,
    exceeded: bool,
}

/// Run a cluster-scale benchmark: create `num_nodes` nodes and `num_pods` pods,
/// schedule all pods, return timing.
fn bench_cluster_scale(catalog: &Catalog, num_nodes: u32, num_pods: u32) -> ScaleResult {
    let instance_types = ["m5.xlarge", "m5.2xlarge", "c5.xlarge", "c5.2xlarge"];
    let mut rng = StdRng::seed_from_u64(42);

    let mut state = ClusterState::new();
    let mut engine = Engine::new(TimeMode::Logical);

    // Create nodes
    for _ in 0..num_nodes {
        let it = instance_types[rng.gen_range(0..instance_types.len())];
        state.add_node(instance_to_node(catalog, it));
    }

    // Schedule pod submissions
    for i in 0..num_pods {
        let cpu = rng.gen_range(100..=1000);
        let mem = rng.gen_range(64..=512) as u64 * 1024 * 1024;
        engine.schedule(
            SimTime(i as u64),
            EngineEvent::PodSubmitted(PodSpec {
                requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                owner: OwnerId(i / 10),
                priority: 0,
                labels: LabelSet::default(),
                scheduling_constraints: SchedulingConstraints::default(),
            }),
        );
    }

    let handler = SimHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated)),
        metrics: MetricsCollector::new(MetricsConfig::default()),
    };
    engine.add_handler(Box::new(handler));

    let rss_before = rss_bytes();
    let start = Instant::now();
    let events = engine.run_to_completion(&mut state);
    let wall_secs = start.elapsed().as_secs_f64();
    let rss_after = rss_bytes();

    ScaleResult {
        nodes: num_nodes,
        pods: num_pods,
        events,
        wall_secs,
        rss: rss_after.saturating_sub(rss_before).max(rss_after / 2), // approximate delta
        exceeded: wall_secs > WALL_CLOCK_LIMIT_SECS,
    }
}

struct DurationResult {
    sim_hours: u64,
    events: u64,
    wall_secs: f64,
    rss: u64,
    exceeded: bool,
}

/// Run a duration benchmark: 50 nodes / 200 pods with periodic events over
/// `sim_hours` of simulated wall-clock time.
fn bench_duration(catalog: &Catalog, sim_hours: u64) -> DurationResult {
    let num_nodes = 50u32;
    let num_pods = 200u32;
    let instance_types = ["m5.xlarge", "m5.2xlarge"];
    let mut rng = StdRng::seed_from_u64(42);

    let mut state = ClusterState::new();
    let mut engine = Engine::new(TimeMode::WallClock);

    for _ in 0..num_nodes {
        let it = instance_types[rng.gen_range(0..instance_types.len())];
        state.add_node(instance_to_node(catalog, it));
    }

    // Submit initial pods at t=0
    for i in 0..num_pods {
        let cpu = rng.gen_range(100..=500);
        let mem = rng.gen_range(64..=256) as u64 * 1024 * 1024;
        engine.schedule(
            SimTime(i as u64 * 1_000_000), // stagger slightly
            EngineEvent::PodSubmitted(PodSpec {
                requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                owner: OwnerId(i / 5),
                priority: 0,
                labels: LabelSet::default(),
                scheduling_constraints: SchedulingConstraints::default(),
            }),
        );
    }

    let ns_per_hour: u64 = 3_600_000_000_000;
    let total_ns = sim_hours * ns_per_hour;

    // Schedule periodic metrics snapshots every simulated hour
    for h in 1..=sim_hours {
        engine.schedule(SimTime(h * ns_per_hour), EngineEvent::MetricsSnapshot);
    }

    // Schedule periodic pod churn: every 15 simulated minutes, submit a batch
    let interval_ns: u64 = 15 * 60_000_000_000;
    let mut t = interval_ns;
    let mut owner_counter = (num_pods / 5) + 1;
    while t < total_ns {
        let batch = rng.gen_range(1..=5);
        for _ in 0..batch {
            let cpu = rng.gen_range(100..=500);
            let mem = rng.gen_range(64..=256) as u64 * 1024 * 1024;
            engine.schedule(
                SimTime(t),
                EngineEvent::PodSubmitted(PodSpec {
                    requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                    owner: OwnerId(owner_counter),
                    priority: 0,
                    labels: LabelSet::default(),
                    scheduling_constraints: SchedulingConstraints::default(),
                }),
            );
        }
        owner_counter += 1;
        t += interval_ns;
    }

    let handler = SimHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated)),
        metrics: MetricsCollector::new(MetricsConfig::default()),
    };
    engine.add_handler(Box::new(handler));

    let rss_before = rss_bytes();
    let start = Instant::now();
    let events = engine.run_until(&mut state, SimTime(total_ns));
    let wall_secs = start.elapsed().as_secs_f64();
    let rss_after = rss_bytes();

    DurationResult {
        sim_hours,
        events,
        wall_secs,
        rss: rss_after.saturating_sub(rss_before).max(rss_after / 2),
        exceeded: wall_secs > WALL_CLOCK_LIMIT_SECS,
    }
}

fn main() {
    let catalog = Catalog::embedded().expect("embedded EC2 catalog");

    let mut output = String::new();
    writeln!(output, "KubeSim Scale Performance Benchmark").unwrap();
    writeln!(output, "====================================").unwrap();
    writeln!(output).unwrap();

    // ── Cluster scale benchmarks ────────────────────────────────
    writeln!(output, "## Cluster Scale (single runs, logical time mode)").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "{:<12} {:<12} {:<16} {:<14} {:<14}",
        "Nodes", "Pods", "Events", "Wall-clock", "RSS").unwrap();
    writeln!(output, "{}", "-".repeat(68)).unwrap();

    let scale_tiers: &[(u32, u32)] = &[
        (50, 500),
        (200, 2_000),
        (500, 5_000),
        (1_000, 10_000),
    ];

    let mut scale_exceeded = false;
    for &(nodes, pods) in scale_tiers {
        if scale_exceeded {
            writeln!(output, "{:<12} {:<12} (skipped — prior tier exceeded 60s limit)",
                nodes, pods).unwrap();
            print_line(&output);
            continue;
        }

        eprint!("  Running cluster scale: {} nodes / {} pods ... ", nodes, pods);
        let r = bench_cluster_scale(&catalog, nodes, pods);
        eprintln!("{:.3}s", r.wall_secs);

        writeln!(output, "{:<12} {:<12} {:<16} {:<14} {:<14}",
            r.nodes, r.pods, r.events,
            format!("{:.3}s", r.wall_secs),
            fmt_bytes(r.rss)).unwrap();
        print_line(&output);

        if r.exceeded {
            writeln!(output, "  ⚠ Exceeded 60s wall-clock limit. Stopping scale escalation.").unwrap();
            scale_exceeded = true;
        }
    }

    writeln!(output).unwrap();

    // ── Duration benchmarks ─────────────────────────────────────
    writeln!(output, "## Simulation Duration (50 nodes / 200 pods, wall_clock time mode)").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "{:<14} {:<16} {:<14} {:<14}",
        "Sim Hours", "Events", "Wall-clock", "RSS").unwrap();
    writeln!(output, "{}", "-".repeat(58)).unwrap();

    let duration_tiers: &[u64] = &[1, 6, 24, 72];

    let mut dur_exceeded = false;
    for &hours in duration_tiers {
        if dur_exceeded {
            writeln!(output, "{:<14} (skipped — prior tier exceeded 60s limit)", hours).unwrap();
            print_line(&output);
            continue;
        }

        eprint!("  Running duration: {}h simulated ... ", hours);
        let r = bench_duration(&catalog, hours);
        eprintln!("{:.3}s", r.wall_secs);

        writeln!(output, "{:<14} {:<16} {:<14} {:<14}",
            format!("{}h", r.sim_hours), r.events,
            format!("{:.3}s", r.wall_secs),
            fmt_bytes(r.rss)).unwrap();
        print_line(&output);

        if r.exceeded {
            writeln!(output, "  ⚠ Exceeded 60s wall-clock limit. Stopping duration escalation.").unwrap();
            dur_exceeded = true;
        }
    }

    writeln!(output).unwrap();

    // ── Batch scale benchmarks (10 runs each) ───────────────────
    writeln!(output, "## Batch Scale (10 runs per tier, logical time mode)").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "{:<12} {:<12} {:<12} {:<16} {:<14} {:<14}",
        "Nodes", "Pods", "Runs", "Total Events", "Wall-clock", "RSS").unwrap();
    writeln!(output, "{}", "-".repeat(80)).unwrap();

    let batch_tiers: &[(u32, u32)] = &[
        (50, 500),
        (200, 2_000),
        (500, 5_000),
        (1_000, 10_000),
    ];

    let mut batch_exceeded = false;
    for &(nodes, pods) in batch_tiers {
        if batch_exceeded {
            writeln!(output, "{:<12} {:<12} {:<12} (skipped — prior tier exceeded 60s limit)",
                nodes, pods, 10).unwrap();
            print_line(&output);
            continue;
        }

        eprint!("  Running batch: {} nodes / {} pods x10 ... ", nodes, pods);
        let start = Instant::now();
        let mut total_events = 0u64;
        for seed in 0..10u64 {
            // Vary the RNG seed per run for realistic batch behavior
            let r = bench_cluster_scale_seeded(&catalog, nodes, pods, seed);
            total_events += r.events;
        }
        let wall_secs = start.elapsed().as_secs_f64();
        let rss = rss_bytes();
        eprintln!("{:.3}s", wall_secs);

        writeln!(output, "{:<12} {:<12} {:<12} {:<16} {:<14} {:<14}",
            nodes, pods, 10, total_events,
            format!("{:.3}s", wall_secs),
            fmt_bytes(rss)).unwrap();
        print_line(&output);

        if wall_secs > WALL_CLOCK_LIMIT_SECS {
            writeln!(output, "  ⚠ Exceeded 60s wall-clock limit. Stopping batch escalation.").unwrap();
            batch_exceeded = true;
        }
    }

    // Write results
    let results_path = "results/scale-perf-benchmark.txt";
    fs::create_dir_all("results").ok();
    fs::write(results_path, &output).expect("failed to write results");
    eprintln!("\nResults written to {}", results_path);
}

/// Same as bench_cluster_scale but with configurable seed.
fn bench_cluster_scale_seeded(catalog: &Catalog, num_nodes: u32, num_pods: u32, seed: u64) -> ScaleResult {
    let instance_types = ["m5.xlarge", "m5.2xlarge", "c5.xlarge", "c5.2xlarge"];
    let mut rng = StdRng::seed_from_u64(seed);

    let mut state = ClusterState::new();
    let mut engine = Engine::new(TimeMode::Logical);

    for _ in 0..num_nodes {
        let it = instance_types[rng.gen_range(0..instance_types.len())];
        state.add_node(instance_to_node(catalog, it));
    }

    for i in 0..num_pods {
        let cpu = rng.gen_range(100..=1000);
        let mem = rng.gen_range(64..=512) as u64 * 1024 * 1024;
        engine.schedule(
            SimTime(i as u64),
            EngineEvent::PodSubmitted(PodSpec {
                requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                owner: OwnerId(i / 10),
                priority: 0,
                labels: LabelSet::default(),
                scheduling_constraints: SchedulingConstraints::default(),
            }),
        );
    }

    let handler = SimHandler {
        scheduler: Scheduler::new(SchedulerProfile::with_scoring("default", ScoringStrategy::LeastAllocated)),
        metrics: MetricsCollector::new(MetricsConfig::default()),
    };
    engine.add_handler(Box::new(handler));

    let start = Instant::now();
    let events = engine.run_to_completion(&mut state);
    let wall_secs = start.elapsed().as_secs_f64();

    ScaleResult {
        nodes: num_nodes,
        pods: num_pods,
        events,
        wall_secs,
        rss: 0,
        exceeded: wall_secs > WALL_CLOCK_LIMIT_SECS,
    }
}

/// Print the last line of output to stderr for progress.
fn print_line(output: &str) {
    if let Some(line) = output.lines().last() {
        eprintln!("    {}", line);
    }
}
