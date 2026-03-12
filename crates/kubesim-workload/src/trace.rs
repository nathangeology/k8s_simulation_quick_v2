//! Trace replay mode — load real cluster traces and convert to DES events.
//!
//! Supports two formats:
//! 1. Prometheus CSV exports: timestamp,pod,cpu_usage,memory_usage
//! 2. Kubernetes event logs: JSON lines with pod lifecycle events

use kubesim_core::{Resources, SimTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::events::Event;
use crate::loader::LoadError;

// ── Prometheus CSV format ───────────────────────────────────────

/// A single row from a Prometheus metric CSV export.
#[derive(Debug, Clone)]
struct PrometheusRow {
    timestamp_ns: u64,
    pod: String,
    cpu_millis: u64,
    memory_bytes: u64,
}

/// Parse a Prometheus CSV string (header: timestamp,pod,cpu_usage,memory_usage).
/// cpu_usage is in cores (float), memory_usage is in bytes (integer).
fn parse_prometheus_csv(csv: &str) -> Result<Vec<PrometheusRow>, LoadError> {
    let mut rows = Vec::new();
    for (i, line) in csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || i == 0 {
            continue; // skip header
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 4 {
            return Err(LoadError::Invalid(format!("CSV line {}: expected 4 columns", i + 1)));
        }
        let timestamp_ns: u64 = cols[0].trim().parse::<f64>()
            .map(|t| (t * 1_000_000_000.0) as u64)
            .map_err(|_| LoadError::Invalid(format!("CSV line {}: bad timestamp", i + 1)))?;
        let pod = cols[1].trim().to_string();
        let cpu_millis = cols[2].trim().parse::<f64>()
            .map(|c| (c * 1000.0) as u64)
            .map_err(|_| LoadError::Invalid(format!("CSV line {}: bad cpu_usage", i + 1)))?;
        let memory_bytes: u64 = cols[3].trim().parse::<f64>()
            .map(|m| m as u64)
            .map_err(|_| LoadError::Invalid(format!("CSV line {}: bad memory_usage", i + 1)))?;
        rows.push(PrometheusRow { timestamp_ns, pod, cpu_millis, memory_bytes });
    }
    Ok(rows)
}

// ── Kubernetes event log format ─────────────────────────────────

/// A single Kubernetes event from a JSON lines export.
#[derive(Debug, Clone, Deserialize)]
struct K8sEvent {
    /// Timestamp in seconds (epoch float).
    timestamp: f64,
    /// Event kind: "create", "delete", "scale".
    kind: String,
    /// Pod or workload name.
    #[serde(default)]
    pod: Option<String>,
    #[serde(default)]
    workload: Option<String>,
    /// For scale events: target replica count.
    #[serde(default)]
    replicas: Option<u32>,
    /// Resource requests (optional, for create events).
    #[serde(default)]
    cpu: Option<f64>,
    #[serde(default)]
    memory: Option<f64>,
}

fn parse_k8s_events(jsonl: &str) -> Result<Vec<K8sEvent>, LoadError> {
    jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| LoadError::Invalid(format!("JSON parse: {e}"))))
        .collect()
}

// ── Trace loading public API ────────────────────────────────────

/// Trace format discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceFormat {
    PrometheusCsv,
    K8sEvents,
}

/// Load a trace file and convert to DES events.
pub fn load_trace(path: &Path, format: TraceFormat) -> Result<Vec<Event>, LoadError> {
    let contents = std::fs::read_to_string(path)?;
    load_trace_from_str(&contents, format)
}

/// Load a trace from a string and convert to DES events.
pub fn load_trace_from_str(data: &str, format: TraceFormat) -> Result<Vec<Event>, LoadError> {
    let mut events = match format {
        TraceFormat::PrometheusCsv => prometheus_to_events(data)?,
        TraceFormat::K8sEvents => k8s_events_to_events(data)?,
    };
    events.sort_by_key(|e| e.time());
    Ok(events)
}

// ── Prometheus → DES events ─────────────────────────────────────

/// Convert Prometheus metrics to DES events by detecting scaling changes.
///
/// Groups rows by pod, sorts by time, and emits PodSubmitted at first appearance.
/// Interpolates between data points: when resource usage changes significantly
/// between samples, intermediate TrafficChange events model the ramp.
fn prometheus_to_events(csv: &str) -> Result<Vec<Event>, LoadError> {
    let rows = parse_prometheus_csv(csv)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // Normalize timestamps relative to first observation
    let t0 = rows.iter().map(|r| r.timestamp_ns).min().unwrap_or(0);

    // Group by pod, sorted by time
    let mut by_pod: HashMap<&str, Vec<&PrometheusRow>> = HashMap::new();
    for row in &rows {
        by_pod.entry(&row.pod).or_default().push(row);
    }

    let mut events = Vec::new();
    let mut owner_counter: u32 = 0;

    for (pod_name, mut samples) in by_pod {
        samples.sort_by_key(|r| r.timestamp_ns);
        let owner_id = owner_counter;
        owner_counter += 1;

        // Emit PodSubmitted at first sample
        let first = samples[0];
        events.push(Event::PodSubmitted {
            time: SimTime(first.timestamp_ns - t0),
            workload_name: pod_name.to_string(),
            owner_id,
            requests: Resources {
                cpu_millis: first.cpu_millis,
                memory_bytes: first.memory_bytes,
                gpu: 0,
                ephemeral_bytes: 0,
            },
            limits: Resources {
                cpu_millis: first.cpu_millis,
                memory_bytes: first.memory_bytes,
                gpu: 0,
                ephemeral_bytes: 0,
            },
            priority: 0,
            deletion_cost: None,
            duration_ns: None,
        });

        // Interpolate between consecutive samples for continuous workload
        for pair in samples.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.cpu_millis == 0 {
                continue;
            }
            let ratio = b.cpu_millis as f64 / a.cpu_millis as f64;
            // Only emit interpolation if usage changed >10%
            if (ratio - 1.0).abs() > 0.1 {
                let mid_t = (a.timestamp_ns + b.timestamp_ns) / 2;
                let mid_ratio = 1.0 + (ratio - 1.0) * 0.5;
                // Midpoint interpolation event
                events.push(Event::TrafficChange {
                    time: SimTime(mid_t - t0),
                    multiplier: mid_ratio,
                });
                // Endpoint event
                events.push(Event::TrafficChange {
                    time: SimTime(b.timestamp_ns - t0),
                    multiplier: ratio,
                });
            }
        }
    }

    Ok(events)
}

// ── K8s events → DES events ─────────────────────────────────────

fn k8s_events_to_events(jsonl: &str) -> Result<Vec<Event>, LoadError> {
    let k8s_events = parse_k8s_events(jsonl)?;
    if k8s_events.is_empty() {
        return Ok(Vec::new());
    }

    let t0 = k8s_events
        .iter()
        .map(|e| e.timestamp)
        .fold(f64::INFINITY, f64::min);

    let mut events = Vec::new();
    let mut owner_map: HashMap<String, u32> = HashMap::new();
    let mut next_owner: u32 = 0;

    for ev in &k8s_events {
        let time_ns = ((ev.timestamp - t0) * 1_000_000_000.0) as u64;
        let name = ev.pod.as_deref().or(ev.workload.as_deref()).unwrap_or("unknown");

        match ev.kind.as_str() {
            "create" => {
                let owner_id = *owner_map.entry(name.to_string()).or_insert_with(|| {
                    let id = next_owner;
                    next_owner += 1;
                    id
                });
                let cpu = ev.cpu.map(|c| (c * 1000.0) as u64).unwrap_or(500);
                let mem = ev.memory.map(|m| m as u64).unwrap_or(512 * 1024 * 1024);
                events.push(Event::PodSubmitted {
                    time: SimTime(time_ns),
                    workload_name: name.to_string(),
                    owner_id,
                    requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                    limits: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
                    priority: 0,
                    deletion_cost: None,
                    duration_ns: None,
                });
            }
            "scale" => {
                if let Some(replicas) = ev.replicas {
                    let owner_id = *owner_map.entry(name.to_string()).or_insert_with(|| {
                        let id = next_owner;
                        next_owner += 1;
                        id
                    });
                    // Emit one PodSubmitted per new replica to model scale-up
                    // (scale-down would need a PodDeleted event which doesn't exist yet,
                    //  so we approximate by emitting HPA evaluation)
                    events.push(Event::HpaEvaluation {
                        time: SimTime(time_ns),
                        owner_id,
                    });
                    // Also emit traffic change to reflect the scaling signal
                    events.push(Event::TrafficChange {
                        time: SimTime(time_ns),
                        multiplier: replicas as f64,
                    });
                }
            }
            "delete" => {
                // Model as traffic drop — the pod is going away
                events.push(Event::TrafficChange {
                    time: SimTime(time_ns),
                    multiplier: 0.0,
                });
            }
            other => {
                // Unknown event kinds are silently skipped
                let _ = other;
            }
        }
    }

    Ok(events)
}
