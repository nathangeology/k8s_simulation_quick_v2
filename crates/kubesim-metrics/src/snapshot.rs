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
    /// Cumulative pod disruptions (consolidation evictions + spot interruptions only).
    /// Scale-down terminations are excluded.
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
    /// Shannon entropy of pod distribution across nodes (raw, in nats).
    pub pod_placement_entropy: f64,
    /// Normalized Shannon entropy (H/ln(N)), 0=concentrated, 1=uniform.
    pub pod_placement_entropy_normalized: f64,
    /// Shannon entropy of CPU allocation across nodes (raw, in nats).
    pub cpu_weighted_entropy: f64,
    /// Normalized CPU-weighted entropy (H/ln(N)).
    pub cpu_weighted_entropy_normalized: f64,
    /// Total vCPU allocated across all nodes (in whole vCPUs, e.g. 4.0 = 4 vCPU).
    pub total_vcpu_allocated: f64,
    /// Total memory allocated across all nodes (in GiB).
    pub total_memory_allocated_gib: f64,
    /// Consolidation decisions evaluated this interval.
    pub consolidation_decisions_total: u32,
    /// Consolidation candidates accepted (ratio >= threshold).
    pub consolidation_decisions_accepted: u32,
    /// Consolidation candidates rejected (ratio < threshold).
    pub consolidation_decisions_rejected: u32,
    /// Mean decision ratio across evaluated candidates.
    pub consolidation_decision_ratio_mean: f64,
    /// Cumulative scale-down terminations (deployment-initiated, NOT disruptions).
    pub scale_down_terminations: u64,
    /// Cumulative consolidation evictions (Karpenter-initiated disruptions).
    pub consolidation_evictions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_empty() {
        let p = Percentiles::from_sorted(&[]);
        assert_eq!(p.p50, 0.0);
        assert_eq!(p.p90, 0.0);
        assert_eq!(p.p99, 0.0);
    }

    #[test]
    fn percentiles_single_value() {
        let p = Percentiles::from_sorted(&[42.0]);
        assert_eq!(p.p50, 42.0);
        assert_eq!(p.p90, 42.0);
        assert_eq!(p.p99, 42.0);
    }

    #[test]
    fn percentiles_multiple_values() {
        let data: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let p = Percentiles::from_sorted(&data);
        assert!((p.p50 - 50.0).abs() < 2.0);
        assert!((p.p90 - 90.0).abs() < 2.0);
        assert!((p.p99 - 99.0).abs() < 2.0);
    }
}
