//! Core metrics collector — implements EventHandler for the DES engine.

use kubesim_core::{ClusterState, PodId, PodPhase, SimTime};
use kubesim_engine::{Event, EventHandler, ScheduledEvent};
use std::collections::HashMap;

use crate::config::{DetailLevel, ExportFormat, MetricsConfig};
use crate::snapshot::{MetricsSnapshot, Percentiles};

/// Compute Shannon entropy and its normalized form from a slice of counts.
/// Returns (raw_entropy, normalized_entropy). If fewer than 2 items or total is 0,
/// returns (0.0, 0.0).
fn shannon_entropy(counts: &[f64]) -> (f64, f64) {
    let total: f64 = counts.iter().sum();
    if total == 0.0 || counts.len() < 2 {
        return (0.0, 0.0);
    }
    let h: f64 = counts.iter()
        .filter(|&&c| c > 0.0)
        .map(|&c| { let p = c / total; -p * p.ln() })
        .sum();
    let max_h = (counts.len() as f64).ln();
    let normalized = if max_h > 0.0 { h / max_h } else { 0.0 };
    (h, normalized)
}

/// Adaptive metrics collector that hooks into the DES event loop.
pub struct MetricsCollector {
    config: MetricsConfig,
    /// Collected snapshots over time.
    snapshots: Vec<MetricsSnapshot>,
    /// Cumulative disruption count.
    disruption_count: u64,
    /// Tracks when each pod entered Pending (for scheduling latency).
    pending_since: HashMap<PodId, SimTime>,
    /// Recent scheduling latencies (cleared each snapshot).
    recent_latencies: Vec<u64>,
}

impl MetricsCollector {
    pub fn new(config: MetricsConfig) -> Self {
        Self {
            config,
            snapshots: Vec::new(),
            disruption_count: 0,
            pending_since: HashMap::new(),
            recent_latencies: Vec::new(),
        }
    }

    /// All collected snapshots.
    pub fn snapshots(&self) -> &[MetricsSnapshot] {
        &self.snapshots
    }

    /// Total disruptions observed.
    pub fn disruption_count(&self) -> u64 {
        self.disruption_count
    }

    /// Export snapshots as JSON string.
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.snapshots)
    }

    /// Export snapshots as CSV string.
    pub fn export_csv(&self) -> String {
        let mut out = String::from(
            "time,total_cost_per_hour,disruption_count,sched_lat_p50,sched_lat_p90,sched_lat_p99,\
             cpu_p50,cpu_p90,cpu_p99,mem_p50,mem_p90,mem_p99,availability,node_count,pod_count,pending_count,detail_level\n",
        );
        for s in &self.snapshots {
            out.push_str(&format!(
                "{},{:.4},{},{:.1},{:.1},{:.1},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{},{}\n",
                s.time.0,
                s.total_cost_per_hour,
                s.disruption_count,
                s.scheduling_latency.p50,
                s.scheduling_latency.p90,
                s.scheduling_latency.p99,
                s.cpu_utilization.p50,
                s.cpu_utilization.p90,
                s.cpu_utilization.p99,
                s.memory_utilization.p50,
                s.memory_utilization.p90,
                s.memory_utilization.p99,
                s.availability,
                s.node_count,
                s.pod_count,
                s.pending_count,
                s.detail_level,
            ));
        }
        out
    }

    /// Export in the configured format.
    pub fn export(&self) -> String {
        match self.config.export_format {
            ExportFormat::Json => self.export_json().unwrap_or_default(),
            ExportFormat::Csv | ExportFormat::Parquet => self.export_csv(),
        }
    }

    /// Take a snapshot of current cluster state.
    fn take_snapshot(&mut self, time: SimTime, state: &ClusterState) {
        let pod_count = state.pods.len();
        let detail_level = self.config.detail_level.resolve(pod_count);

        // Node utilization
        let mut cpu_utils = Vec::new();
        let mut mem_utils = Vec::new();
        let mut total_cost = 0.0f64;
        let mut node_count = 0u32;

        for (_id, node) in state.nodes.iter() {
            node_count += 1;
            total_cost += node.cost_per_hour;
            let cpu_util = if node.allocatable.cpu_millis > 0 {
                node.allocated.cpu_millis as f64 / node.allocatable.cpu_millis as f64
            } else {
                0.0
            };
            let mem_util = if node.allocatable.memory_bytes > 0 {
                node.allocated.memory_bytes as f64 / node.allocatable.memory_bytes as f64
            } else {
                0.0
            };
            cpu_utils.push(cpu_util);
            mem_utils.push(mem_util);
        }

        cpu_utils.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        mem_utils.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Pod counts and availability
        let mut running = 0u32;
        let mut active = 0u32; // non-terminal pods
        let mut pending = 0u32;

        for (_id, pod) in state.pods.iter() {
            match pod.phase {
                PodPhase::Running => {
                    running += 1;
                    active += 1;
                }
                PodPhase::Pending => {
                    pending += 1;
                    active += 1;
                }
                PodPhase::Terminating => {
                    active += 1;
                }
                PodPhase::Succeeded | PodPhase::Failed => {}
            }
        }

        let availability = if active > 0 {
            running as f64 / active as f64
        } else {
            1.0
        };

        // Scheduling latency from recent observations
        let mut lat_sorted: Vec<f64> = self.recent_latencies.iter().map(|&l| l as f64).collect();
        lat_sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let detail_str = match detail_level {
            DetailLevel::Pod => "pod",
            DetailLevel::Deployment => "deployment",
            DetailLevel::Namespace => "namespace",
            DetailLevel::Cluster => "cluster",
            DetailLevel::Auto => "auto",
        };

        // Pod placement entropy
        let pod_counts: Vec<f64> = state.nodes.iter()
            .map(|(_id, node)| node.pods.len() as f64)
            .collect();
        let (pod_placement_entropy, pod_placement_entropy_normalized) = shannon_entropy(&pod_counts);

        // CPU-weighted entropy
        let cpu_allocs: Vec<f64> = state.nodes.iter()
            .map(|(_id, node)| node.allocated.cpu_millis as f64)
            .collect();
        let (cpu_weighted_entropy, cpu_weighted_entropy_normalized) = shannon_entropy(&cpu_allocs);

        self.snapshots.push(MetricsSnapshot {
            time,
            total_cost_per_hour: total_cost,
            disruption_count: self.disruption_count,
            scheduling_latency: Percentiles::from_sorted(&lat_sorted),
            cpu_utilization: Percentiles::from_sorted(&cpu_utils),
            memory_utilization: Percentiles::from_sorted(&mem_utils),
            availability,
            node_count,
            pod_count,
            pending_count: pending,
            detail_level: detail_str.to_string(),
            pod_placement_entropy,
            pod_placement_entropy_normalized,
            cpu_weighted_entropy,
            cpu_weighted_entropy_normalized,
        });

        self.recent_latencies.clear();
    }
}

impl EventHandler for MetricsCollector {
    fn handle(
        &mut self,
        event: &kubesim_engine::Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        match event {
            Event::PodSubmitted(_) => {
                // Track pending time for the most recently added pod.
                // The pod was just inserted into state by a prior handler,
                // so grab the last pending pod.
                if let Some(&pod_id) = state.pending_queue.last() {
                    self.pending_since.insert(pod_id, time);
                }
            }
            Event::PodScheduled(pod_id, _) | Event::PodRunning(pod_id) => {
                if let Some(start) = self.pending_since.remove(pod_id) {
                    let latency = time.0.saturating_sub(start.0);
                    self.recent_latencies.push(latency);
                }
            }
            Event::PodTerminating(_) | Event::PodDeleted(_) => {
                // Disruption: pod removed unexpectedly.
                self.disruption_count += 1;
            }
            Event::SpotInterruption(_) => {
                self.disruption_count += 1;
            }
            Event::NodeDrained(_) | Event::NodeTerminated(_) => {
                // Node-level disruptions tracked via pod events above.
            }
            Event::MetricsSnapshot => {
                self.take_snapshot(time, state);
            }
            _ => {}
        }
        Vec::new()
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}
#[cfg(test)]
mod tests {
    use super::*;
    use kubesim_core::*;
    use kubesim_engine::EventHandler;

    fn test_node(cpu: u64, mem: u64) -> Node {
        Node {
            instance_type: "m5.xlarge".into(),
            allocatable: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            allocated: Resources::default(),
            pods: smallvec::smallvec![],
            conditions: NodeConditions { ready: true, ..Default::default() },
            labels: LabelSet::default(),
            taints: smallvec::smallvec![],
            cost_per_hour: 0.192,
            lifecycle: NodeLifecycle::OnDemand,
            cordoned: false,
            created_at: SimTime(0),
            pool_name: String::new(),
            do_not_disrupt: false,
        }
    }

    #[test]
    fn snapshot_captures_cluster_state() {
        let mut state = ClusterState::new();
        state.add_node(test_node(4000, 8_000_000_000));

        let mut collector = MetricsCollector::new(MetricsConfig::default());
        collector.handle(&Event::MetricsSnapshot, SimTime(1000), &mut state);

        assert_eq!(collector.snapshots().len(), 1);
        let snap = &collector.snapshots()[0];
        assert_eq!(snap.node_count, 1);
        assert_eq!(snap.time, SimTime(1000));
        assert!((snap.total_cost_per_hour - 0.192).abs() < 0.001);
    }

    #[test]
    fn disruption_count_incremented() {
        let mut state = ClusterState::new();
        let nid = state.add_node(test_node(4000, 8_000_000_000));

        let mut collector = MetricsCollector::new(MetricsConfig::default());
        collector.handle(&Event::SpotInterruption(nid), SimTime(100), &mut state);

        assert_eq!(collector.disruption_count(), 1);
    }

    #[test]
    fn export_csv_has_header() {
        let collector = MetricsCollector::new(MetricsConfig { export_format: crate::config::ExportFormat::Csv, ..Default::default() });
        let csv = collector.export_csv();
        assert!(csv.starts_with("time,"));
    }

    #[test]
    fn export_json_valid() {
        let mut state = ClusterState::new();
        state.add_node(test_node(4000, 8_000_000_000));

        let mut collector = MetricsCollector::new(MetricsConfig { export_format: crate::config::ExportFormat::Json, ..Default::default() });
        collector.handle(&Event::MetricsSnapshot, SimTime(1000), &mut state);

        let json = collector.export();
        assert!(json.contains("total_cost_per_hour"));
        // Verify it's valid JSON
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&json);
        assert!(parsed.is_ok());
    }
}
