//! Metrics snapshot — the output of a collection interval.

use kubesim_core::SimTime;
use serde::{Deserialize, Serialize};

/// Percentile values for a distribution.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Percentiles {
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
}

impl Percentiles {
    /// Compute percentiles from a sorted slice of values.
    pub fn from_sorted(sorted: &[f64]) -> Self {
        if sorted.is_empty() {
            return Self::default();
        }
        Self {
            p50: percentile_sorted(sorted, 0.50),
            p90: percentile_sorted(sorted, 0.90),
            p99: percentile_sorted(sorted, 0.99),
        }
    }
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// A point-in-time metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Simulation time of this snapshot.
    pub time: SimTime,
    /// Total hourly cost of all running nodes.
    pub total_cost_per_hour: f64,
    /// Cumulative pod disruptions (evictions + preemptions + spot interruptions).
    pub disruption_count: u64,
    /// Scheduling latency percentiles (pending → running, in SimTime nanos).
    pub scheduling_latency: Percentiles,
    /// CPU utilization percentiles across nodes (0.0–1.0).
    pub cpu_utilization: Percentiles,
    /// Memory utilization percentiles across nodes (0.0–1.0).
    pub memory_utilization: Percentiles,
    /// Availability: fraction of pods in Running state vs total non-succeeded/failed.
    pub availability: f64,
    /// Number of active nodes.
    pub node_count: u32,
    /// Number of active pods.
    pub pod_count: u32,
    /// Number of pending pods.
    pub pending_count: u32,
    /// Effective detail level used for this snapshot.
    pub detail_level: String,
}
