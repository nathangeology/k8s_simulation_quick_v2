//! YAML scenario data model.
//!
//! Supports the two study formats from ARCHITECTURE.md:
//! - scheduling-comparison (MostAllocated vs LeastAllocated)
//! - deletion-cost-drain (pod deletion cost strategies)

use serde::{Deserialize, Serialize};

pub use kubesim_core::DeletionCostStrategy;
pub use kubesim_ec2::CatalogProvider;

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
    #[serde(default)]
    pub catalog_provider: CatalogProvider,
    #[serde(default)]
    pub scheduling_strategy: SchedulingStrategy,
    pub cluster: ClusterConfig,
    pub workloads: Vec<WorkloadDef>,
    #[serde(default)]
    pub traffic_pattern: Option<TrafficPattern>,
    #[serde(default)]
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub measurement_windows: Vec<MeasurementWindow>,
}

/// Scheduling strategy controlling how SimHandler schedules pods on NodeReady.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SchedulingStrategy {
    /// Run full filter chain for all pending pods (current behavior, faithful to real k8s).
    #[default]
    FullScan,
    /// Provisioner passes pod-to-node hints, scheduler binds directly on NodeReady.
    HintBased,
    /// Partition pods by constraint class, only try relevant pods per NodeReady.
    Partitioned,
    /// Like FullScan but skips saturated nodes in the filter loop.
    NodePruning,
    /// On NodeReady, only evaluate the new node instead of scanning all nodes.
    ReverseSchedule,
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
    /// Fixed system overhead subtracted from every node's allocatable resources.
    #[serde(default)]
    pub system_overhead: Option<SystemOverhead>,
    /// Percentage of node capacity reserved for daemonsets (applied after fixed overhead).
    #[serde(default)]
    pub daemonset_overhead_percent: Option<u32>,
    /// Daemonset pods created on every node at NodeReady.
    #[serde(default)]
    pub daemonsets: Option<Vec<DaemonSetDef>>,
    /// Action delays to model real-world latency.
    #[serde(default)]
    pub delays: ActionDelays,
}

/// Configurable delays for realistic timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDelays {
    /// Time from NodeLaunching to NodeReady (default "0s").
    #[serde(default = "default_zero_duration")]
    pub node_startup: String,
    /// Jitter range for node startup delay (uniform ±jitter). Optional.
    #[serde(default)]
    pub node_startup_jitter: Option<String>,
    /// Time from NodeDrained to NodeTerminated (default "0s").
    #[serde(default = "default_zero_duration")]
    pub node_shutdown: String,
    /// Jitter range for node shutdown delay (uniform ±jitter). Optional.
    #[serde(default)]
    pub node_shutdown_jitter: Option<String>,
    /// Provisioner batch window — delay before first provisioning pass (default "0s").
    #[serde(default = "default_zero_duration")]
    pub provisioner_batch: String,
    /// Jitter range for provisioner batch window (uniform ±jitter). Optional.
    #[serde(default)]
    pub provisioner_batch_jitter: Option<String>,
    /// Time for a pod to transition from Pending to Running once bound (default "0s").
    #[serde(default = "default_zero_duration")]
    pub pod_startup: String,
    /// Jitter range for pod startup delay (uniform ±jitter). Optional.
    #[serde(default)]
    pub pod_startup_jitter: Option<String>,
}

fn default_zero_duration() -> String { "0s".to_string() }

impl Default for ActionDelays {
    fn default() -> Self {
        Self {
            node_startup: "0s".to_string(),
            node_startup_jitter: None,
            node_shutdown: "0s".to_string(),
            node_shutdown_jitter: None,
            provisioner_batch: "0s".to_string(),
            provisioner_batch_jitter: None,
            pod_startup: "0s".to_string(),
            pod_startup_jitter: None,
        }
    }
}

impl ActionDelays {
    pub fn node_startup_ns(&self) -> u64 { parse_duration_ns(&self.node_startup).unwrap_or(0) }
    pub fn node_startup_jitter_ns(&self) -> u64 { self.node_startup_jitter.as_deref().and_then(parse_duration_ns).unwrap_or(0) }
    pub fn node_shutdown_ns(&self) -> u64 { parse_duration_ns(&self.node_shutdown).unwrap_or(0) }
    pub fn node_shutdown_jitter_ns(&self) -> u64 { self.node_shutdown_jitter.as_deref().and_then(parse_duration_ns).unwrap_or(0) }
    pub fn provisioner_batch_ns(&self) -> u64 { parse_duration_ns(&self.provisioner_batch).unwrap_or(0) }
    pub fn provisioner_batch_jitter_ns(&self) -> u64 { self.provisioner_batch_jitter.as_deref().and_then(parse_duration_ns).unwrap_or(0) }
    pub fn pod_startup_ns(&self) -> u64 { parse_duration_ns(&self.pod_startup).unwrap_or(0) }
    pub fn pod_startup_jitter_ns(&self) -> u64 { self.pod_startup_jitter.as_deref().and_then(parse_duration_ns).unwrap_or(0) }
}

/// A daemonset definition in scenario YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSetDef {
    pub name: String,
    #[serde(default = "default_ds_cpu")]
    pub cpu_request: String,
    #[serde(default = "default_ds_memory")]
    pub memory_request: String,
}

fn default_ds_cpu() -> String { "150m".into() }
fn default_ds_memory() -> String { "500Mi".into() }

impl DaemonSetDef {
    pub fn cpu_millis(&self) -> u64 {
        parse_cpu_millis(&self.cpu_request).unwrap_or(150)
    }
    pub fn memory_bytes(&self) -> u64 {
        parse_memory_bytes(&self.memory_request).unwrap_or(500 * 1024 * 1024)
    }
}

/// Fixed resource overhead subtracted from node allocatable capacity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOverhead {
    #[serde(default = "default_overhead_cpu")]
    pub cpu: String,
    #[serde(default = "default_overhead_memory")]
    pub memory: String,
}

impl Default for SystemOverhead {
    fn default() -> Self {
        Self {
            cpu: "250m".into(),
            memory: "500Mi".into(),
        }
    }
}

fn default_overhead_cpu() -> String { "250m".into() }
fn default_overhead_memory() -> String { "500Mi".into() }

impl SystemOverhead {
    pub fn cpu_millis(&self) -> u64 {
        parse_cpu_millis(&self.cpu).unwrap_or(250)
    }
    pub fn memory_bytes(&self) -> u64 {
        parse_memory_bytes(&self.memory).unwrap_or(500 * 1024 * 1024)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePoolDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(deserialize_with = "deserialize_instance_types")]
    pub instance_types: Vec<String>,
    #[serde(default = "default_min_nodes")]
    pub min_nodes: u32,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: u32,
    #[serde(default)]
    pub labels: Vec<(String, String)>,
    #[serde(default)]
    pub taints: Vec<kubesim_core::Taint>,
    #[serde(default)]
    pub weight: u32,
    #[serde(default)]
    pub karpenter: Option<KarpenterConfig>,
    #[serde(default)]
    pub disruption_budget: Option<DisruptionBudgetDef>,
    /// When true, nodes in this pool have `karpenter.sh/do-not-disrupt` annotation.
    #[serde(default)]
    pub do_not_disrupt: bool,
}

/// Disruption budget configuration from scenario YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisruptionBudgetDef {
    /// Maximum percentage of nodes that may be disrupted (default: 10).
    #[serde(default = "default_disruption_max_percent")]
    pub max_percent: u32,
    /// Absolute count cap (optional, overrides percentage when set).
    #[serde(default)]
    pub max_count: Option<u32>,
    /// Cron schedule for time-gated overrides (v1.x only, optional).
    #[serde(default)]
    pub schedule: Option<String>,
    /// Budget percentage when schedule is active (v1.x only, optional).
    #[serde(default)]
    pub active_budget: Option<u32>,
    /// Budget percentage when schedule is inactive (v1.x only, optional).
    #[serde(default)]
    pub inactive_budget: Option<u32>,
}

fn default_disruption_max_percent() -> u32 { 10 }

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
    /// Decision ratio threshold for WhenCostJustifiesDisruption (default 1.0).
    #[serde(default)]
    pub decision_ratio_threshold: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsolidationPolicy {
    WhenEmpty,
    WhenUnderutilized,
    WhenEmptyOrUnderutilized,
    WhenCostJustifiesDisruption,
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
    pub pod_anti_affinity: Option<PodAntiAffinityDef>,
    #[serde(default)]
    pub node_selector: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub labels: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub pdb: Option<PdbDef>,
    #[serde(default)]
    pub churn: Option<ChurnLevel>,
    #[serde(default)]
    pub traffic: Option<String>,
    /// Scale-down events: reduce replicas at specified times.
    #[serde(default)]
    pub scale_down: Option<Vec<ScaleDownEvent>>,
    /// Per-instance stagger interval for scale-down when count > 1 (e.g. "5m", "10m").
    /// Defaults to "5m" if not specified.
    #[serde(default)]
    pub scale_down_stagger: Option<String>,
    /// When this workload begins (e.g. "0s", "5m", "1h"). Default: "0s".
    #[serde(default)]
    pub start_at: Option<String>,
    /// Time between individual pod submissions within a deployment (e.g. "1s", "2s").
    /// Simulates rolling deployment. Default: no stagger (all at once).
    #[serde(default)]
    pub pod_submit_interval: Option<String>,
    /// Time between individual pod removals during scale-down (e.g. "2s").
    /// Simulates RS controller removing pods one at a time. Default: all at once.
    #[serde(default)]
    pub scale_down_interval: Option<String>,
    /// Scale-up events: increase replicas at specified times.
    #[serde(default)]
    pub scale_up: Option<Vec<ScaleUpEvent>>,
    /// In-place vertical scaling resource changes at specified times.
    #[serde(default)]
    pub resource_changes: Option<Vec<ResourceChangeEvent>>,
    /// Resize policy for in-place vertical scaling (default: InPlace).
    #[serde(default)]
    pub resize_policy: Option<String>,
}

/// A scale-down event that reduces replicas at a given time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleDownEvent {
    /// Time offset (e.g. "12h", "30m", "3600s") from simulation start.
    pub at: String,
    /// Number of replicas to reduce by.
    pub reduce_by: u32,
}

/// A scale-up event that increases replicas at a given time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleUpEvent {
    /// Time offset (e.g. "10s", "5m") from simulation start.
    pub at: String,
    /// Target replica count to scale up to.
    pub increase_to: u32,
}

/// An in-place resource change event for vertical scaling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceChangeEvent {
    /// Time offset (e.g. "2m", "5m") from simulation start.
    pub at: String,
    /// New CPU request (e.g. "1000m", "500m").
    #[serde(default)]
    pub cpu_request: Option<String>,
    /// New memory request (e.g. "2Gi", "1Gi").
    #[serde(default)]
    pub memory_request: Option<String>,
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

pub(crate) fn parse_cpu_millis(s: &str) -> Option<u64> {
    if let Some(v) = s.strip_suffix('m') {
        v.parse::<u64>().ok()
    } else {
        s.parse::<f64>().ok().map(|v| (v * 1000.0) as u64)
    }
}

pub(crate) fn parse_memory_bytes(s: &str) -> Option<u64> {
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
pub struct PodAntiAffinityDef {
    /// Label key to match against (e.g. "app")
    pub label_key: String,
    /// Topology key (e.g. "kubernetes.io/hostname")
    pub topology_key: String,
    /// "required" or "preferred" (default: "preferred")
    #[serde(default = "default_affinity_type")]
    pub affinity_type: String,
    /// Weight for preferred anti-affinity (default: 100)
    #[serde(default = "default_affinity_weight")]
    pub weight: u32,
    /// Override selector value for cross anti-affinity.
    /// If set, the anti-affinity selector uses this value instead of the pod's own label.
    #[serde(default)]
    pub target_label_value: Option<String>,
}

fn default_affinity_type() -> String { "preferred".into() }
fn default_affinity_weight() -> u32 { 100 }

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
    /// Per-variant disruption budget override (overrides pool-level budget).
    #[serde(default)]
    pub disruption_budget: Option<DisruptionBudgetDef>,
    /// Per-variant consolidation policy override.
    #[serde(default)]
    pub consolidate_when: Option<ConsolidateWhenVariant>,
}

/// Per-variant consolidation policy override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidateWhenVariant {
    pub policy: ConsolidationPolicy,
    #[serde(default)]
    pub decision_ratio_threshold: Option<f64>,
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

// ── Measurement windows ─────────────────────────────────────────

/// A named time window for per-transition metrics comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementWindow {
    pub name: String,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub description: Option<String>,
}

// ── Instance type shorthand expansion ───────────────────────────

/// Deserialize `instance_types` as either a single string shorthand or a list of strings.
fn deserialize_instance_types<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Single(String),
        List(Vec<String>),
    }
    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::Single(s) => Ok(vec![s]),
        StringOrVec::List(v) => Ok(v),
    }
}

/// Expand shorthand instance type values (`all-ec2`, `all-kwok`) into full catalog lists.
/// Returns an error if a shorthand catalog fails to load.
pub fn resolve_instance_type_shorthands(study: &mut Study) -> Result<(), String> {
    for pool in &mut study.cluster.node_pools {
        if pool.instance_types.len() == 1 {
            match pool.instance_types[0].as_str() {
                "all-ec2" => {
                    let catalog = kubesim_ec2::Catalog::embedded()
                        .map_err(|e| format!("failed to load EC2 catalog: {e}"))?;
                    pool.instance_types = catalog.all().iter()
                        .map(|t| t.instance_type.clone())
                        .collect();
                }
                "all-kwok" => {
                    let catalog = kubesim_ec2::Catalog::kwok()
                        .map_err(|e| format!("failed to load KWOK catalog: {e}"))?;
                    pool.instance_types = catalog.all().iter()
                        .map(|t| t.instance_type.clone())
                        .collect();
                }
                _ => {}
            }
        }
    }
    Ok(())
}
