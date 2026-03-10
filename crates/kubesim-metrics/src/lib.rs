//! KubeSim Metrics — Adaptive metrics collector with configurable detail levels.
//!
//! Collects simulation metrics at configurable granularity. Detail level adapts
//! automatically based on pod count, or can be pinned via configuration.
//!
//! Always-on metrics (regardless of detail level):
//! - Total cost (on-demand + spot, per time unit)
//! - Pod disruption count (evictions, preemptions, spot interruptions)
//! - Scheduling latency (pending → running time)
//! - Node utilization distribution (P50/P90/P99)
//! - Availability (fraction of time desired replicas == ready replicas)

mod collector;
mod config;
mod snapshot;

pub use collector::MetricsCollector;
pub use config::{DetailLevel, ExportFormat, MetricsConfig};
pub use snapshot::{MetricsSnapshot, Percentiles};
