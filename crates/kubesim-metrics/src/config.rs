//! Configuration types for the metrics collector.

use serde::{Deserialize, Serialize};

/// Controls the granularity of collected metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    /// Per-pod, per-event tracking.
    Pod,
    /// Per-deployment aggregation, sampled events.
    Deployment,
    /// Per-namespace aggregation.
    Namespace,
    /// Cluster-wide aggregates only.
    Cluster,
    /// Automatically select based on pod count.
    Auto,
}

impl Default for DetailLevel {
    fn default() -> Self {
        Self::Auto
    }
}

impl DetailLevel {
    /// Resolve `Auto` to a concrete level based on current pod count.
    pub fn resolve(self, pod_count: u32) -> DetailLevel {
        match self {
            Self::Auto => match pod_count {
                0..1_000 => Self::Pod,
                1_000..10_000 => Self::Deployment,
                10_000..100_000 => Self::Namespace,
                _ => Self::Cluster,
            },
            other => other,
        }
    }
}

/// Export format for metrics snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Parquet,
    Csv,
    Json,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Parquet
    }
}

/// Configuration for the metrics collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Detail level for metrics collection.
    #[serde(default)]
    pub detail_level: DetailLevel,
    /// Fraction of events to record (0.0–1.0).
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
    /// Export format for snapshots.
    #[serde(default)]
    pub export_format: ExportFormat,
}

fn default_sample_rate() -> f64 {
    1.0
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            detail_level: DetailLevel::default(),
            sample_rate: 1.0,
            export_format: ExportFormat::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_level_auto_resolves() {
        assert_eq!(DetailLevel::Auto.resolve(500), DetailLevel::Pod);
        assert_eq!(DetailLevel::Auto.resolve(5000), DetailLevel::Deployment);
        assert_eq!(DetailLevel::Auto.resolve(50000), DetailLevel::Namespace);
        assert_eq!(DetailLevel::Auto.resolve(500000), DetailLevel::Cluster);
    }

    #[test]
    fn detail_level_explicit_unchanged() {
        assert_eq!(DetailLevel::Pod.resolve(500000), DetailLevel::Pod);
        assert_eq!(DetailLevel::Cluster.resolve(1), DetailLevel::Cluster);
    }

    #[test]
    fn default_config() {
        let config = MetricsConfig::default();
        assert_eq!(config.detail_level, DetailLevel::Auto);
        assert_eq!(config.sample_rate, 1.0);
        assert_eq!(config.export_format, ExportFormat::Parquet);
    }
}
