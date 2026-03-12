//! YAML scenario data model.
//!
//! Supports the two study formats from ARCHITECTURE.md:
//! - scheduling-comparison (MostAllocated vs LeastAllocated)
//! - deletion-cost-drain (pod deletion cost strategies)

use serde::{Deserialize, Serialize};

pub use kubesim_core::DeletionCostStrategy;

/// Top-level scenario file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioFile {
    pub study: Study,
}

/// A study definition with cluster, workloads, variants, and metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Study {
    pub name: String,
    #[serde(default = "default_runs")]
    pub runs: u32,
    #[serde(default)]
    pub time_mode: TimeMode,
    pub cluster: ClusterConfig,
    pub workloads: Vec<WorkloadDef>,
    #[serde(default)]
    pub traffic_pattern: Option<TrafficPattern>,
    #[serde(default)]
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub metrics: MetricsConfig,
}

fn default_runs() -> u32 {
    1000
}

// ── Time mode ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeMode {
    #[default]
    Logical,
    WallClock,
}

// ── Cluster config ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub node_pools: Vec<NodePoolDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePoolDef {
    #[serde(default)]
    pub name: Option<String>,
    pub instance_types: Vec<String>,
    #[serde(default = "default_min_nodes")]
    pub min_nodes: u32,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: u32,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub taints: Vec<kubesim_core::Taint>,
    #[serde(default)]
    pub weight: u32,
    #[serde(default)]
    pub karpenter: Option<KarpenterConfig>,
}

impl NodePoolDef {
    /// Convert labels map to Vec<(String, String)> for NodePool construction.
    pub fn labels_vec(&self) -> Vec<(String, String)> {
        self.labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

fn default_min_nodes() -> u32 {
    1
}
fn default_max_nodes() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarpenterConfig {
    #[serde(default)]
    pub consolidation: Option<ConsolidationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    pub policy: ConsolidationPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsolidationPolicy {
    WhenEmpty,
    WhenUnderutilized,
}

// ── Workload definitions ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadDef {
    #[serde(rename = "type")]
    pub workload_type: String,
    #[serde(default)]
    pub count: ValueOrDist,
    #[serde(default)]
    pub replicas: Option<ReplicaSpec>,
    #[serde(default)]
    pub scaling: Option<ScalingConfig>,
    #[serde(default)]
    pub cpu_request: Option<Distribution>,
    #[serde(default)]
    pub memory_request: Option<Distribution>,
    #[serde(default)]
    pub gpu_request: Option<Distribution>,
    #[serde(default)]
    pub duration: Option<Distribution>,
    #[serde(default)]
    pub priority: Option<PriorityLevel>,
    #[serde(default)]
    pub topology_spread: Option<TopologySpreadDef>,
    #[serde(default)]
    pub pdb: Option<PdbDef>,
    #[serde(default)]
    pub churn: Option<ChurnLevel>,
    #[serde(default)]
    pub traffic: Option<String>,
    /// Scale-down events: reduce replicas at specified times.
    #[serde(default)]
    pub scale_down: Option<Vec<ScaleDownEvent>>,
}

/// A scale-down event that reduces replicas at a given time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleDownEvent {
    /// Time offset (e.g. "12h", "30m", "3600s") from simulation start.
    pub at: String,
    /// Number of replicas to reduce by.
    pub reduce_by: u32,
}

/// Either a fixed integer or a distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValueOrDist {
    Fixed(u32),
    Dist(Distribution),
}

impl Default for ValueOrDist {
    fn default() -> Self {
        ValueOrDist::Fixed(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaSpec {
    #[serde(default)]
    pub min: Option<u32>,
    #[serde(default)]
    pub max: Option<u32>,
    #[serde(default)]
    pub fixed: Option<u32>,
}

// ── Distributions ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "dist", rename_all = "snake_case")]
pub enum Distribution {
    Uniform {
        min: QuantityValue,
        max: QuantityValue,
    },
    Normal {
        mean: QuantityValue,
        std: QuantityValue,
    },
    Poisson {
        lambda: f64,
    },
    Lognormal {
        mean: QuantityValue,
        std: QuantityValue,
    },
    Exponential {
        mean: QuantityValue,
    },
    Choice {
        values: Vec<QuantityValue>,
    },
}

/// A quantity value that can be a number or a K8s resource string (e.g. "250m", "256Mi").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QuantityValue {
    Float(f64),
    Int(u64),
    Str(String),
}

impl QuantityValue {
    /// Parse to millicores (for CPU quantities like "250m", "4").
    pub fn to_cpu_millis(&self) -> Option<u64> {
        match self {
            QuantityValue::Float(v) => Some((*v * 1000.0) as u64),
            QuantityValue::Int(v) => Some(*v * 1000),
            QuantityValue::Str(s) => parse_cpu_millis(s),
        }
    }

    /// Parse to bytes (for memory quantities like "256Mi", "16Gi").
    pub fn to_memory_bytes(&self) -> Option<u64> {
        match self {
            QuantityValue::Float(v) => Some(*v as u64),
            QuantityValue::Int(v) => Some(*v),
            QuantityValue::Str(s) => parse_memory_bytes(s),
        }
    }

    /// Parse to f64 for generic numeric use.
    pub fn to_f64(&self) -> Option<f64> {
        match self {
            QuantityValue::Float(v) => Some(*v),
            QuantityValue::Int(v) => Some(*v as f64),
            QuantityValue::Str(s) => s.parse().ok(),
        }
    }
}

fn parse_cpu_millis(s: &str) -> Option<u64> {
    if let Some(v) = s.strip_suffix('m') {
        v.parse::<u64>().ok()
    } else {
        s.parse::<f64>().ok().map(|v| (v * 1000.0) as u64)
    }
}

fn parse_memory_bytes(s: &str) -> Option<u64> {
    if let Some(v) = s.strip_suffix("Gi") {
        v.parse::<f64>().ok().map(|v| (v * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if let Some(v) = s.strip_suffix("Mi") {
        v.parse::<f64>().ok().map(|v| (v * 1024.0 * 1024.0) as u64)
    } else if let Some(v) = s.strip_suffix("Ki") {
        v.parse::<f64>().ok().map(|v| (v * 1024.0) as u64)
    } else {
        s.parse::<u64>().ok()
    }
}

pub fn parse_duration_ns(s: &str) -> Option<u64> {
    if let Some(v) = s.strip_suffix('h') {
        v.parse::<f64>().ok().map(|v| (v * 3_600_000_000_000.0) as u64)
    } else if let Some(v) = s.strip_suffix('m') {
        v.parse::<f64>().ok().map(|v| (v * 60_000_000_000.0) as u64)
    } else if let Some(v) = s.strip_suffix('s') {
        v.parse::<f64>().ok().map(|v| (v * 1_000_000_000.0) as u64)
    } else {
        s.parse::<u64>().ok()
    }
}

impl QuantityValue {
    /// Parse to nanoseconds (for duration quantities like "4h", "30m").
    pub fn to_duration_ns(&self) -> Option<u64> {
        match self {
            QuantityValue::Float(v) => Some((*v * 1_000_000_000.0) as u64),
            QuantityValue::Int(v) => Some(*v * 1_000_000_000),
            QuantityValue::Str(s) => parse_duration_ns(s),
        }
    }
}

// ── Scaling ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingConfig {
    #[serde(rename = "type")]
    pub scaling_type: ScalingType,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub target: Option<PercentOrValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalingType {
    Hpa,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PercentOrValue {
    Percent(String),
    Value(f64),
}

// ── Priority ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriorityLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl PriorityLevel {
    pub fn to_i32(self) -> i32 {
        match self {
            PriorityLevel::Low => -100,
            PriorityLevel::Medium => 0,
            PriorityLevel::High => 100,
            PriorityLevel::Critical => 1000,
        }
    }
}

// ── Topology spread / PDB ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySpreadDef {
    pub max_skew: u32,
    pub topology_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdbDef {
    #[serde(default)]
    pub min_available: Option<String>,
    #[serde(default)]
    pub max_unavailable: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChurnLevel {
    Low,
    Medium,
    High,
}

// ── Traffic patterns ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficPattern {
    #[serde(rename = "type")]
    pub pattern_type: String,
    #[serde(default)]
    pub peak_multiplier: Option<f64>,
    #[serde(default)]
    pub duration: Option<String>,
}

// ── Study variants ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    #[serde(default)]
    pub scheduler: Option<SchedulerVariant>,
    #[serde(default)]
    pub deletion_cost_strategy: Option<DeletionCostStrategy>,
    #[serde(default)]
    pub pdb: Option<PdbDef>,
    #[serde(default)]
    pub karpenter_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerVariant {
    pub scoring: ScoringStrategy,
    #[serde(default = "default_weight")]
    pub weight: i64,
}

fn default_weight() -> i64 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoringStrategy {
    MostAllocated,
    LeastAllocated,
}

// DeletionCostStrategy is re-exported from kubesim_core

// ── Metrics config ──────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default)]
    pub compare: Vec<String>,
}
