//! NodePool configuration — defines instance type constraints and limits.

use serde::{Deserialize, Serialize};

/// Limits on what a NodePool can provision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePoolLimits {
    pub max_nodes: Option<u32>,
    pub max_cpu_millis: Option<u64>,
    pub max_memory_bytes: Option<u64>,
}

impl Default for NodePoolLimits {
    fn default() -> Self {
        Self { max_nodes: None, max_cpu_millis: None, max_memory_bytes: None }
    }
}

/// A Karpenter NodePool — constrains which instance types can be launched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePool {
    pub name: String,
    /// Allowed instance type names (empty = all catalog types allowed).
    pub instance_types: Vec<String>,
    pub limits: NodePoolLimits,
    /// Labels applied to every node launched from this pool.
    pub labels: Vec<(String, String)>,
    /// Taints applied to every node launched from this pool.
    pub taints: Vec<kubesim_core::Taint>,
    /// Maximum percentage of nodes that may be disrupted simultaneously (0–100).
    #[serde(default = "default_disruption_pct")]
    pub max_disrupted_pct: u32,
}

fn default_disruption_pct() -> u32 { 10 }

/// Current usage tracked against a NodePool's limits.
#[derive(Debug, Clone, Default)]
pub struct NodePoolUsage {
    pub node_count: u32,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
}

impl NodePool {
    /// Check whether launching an instance with the given resources would exceed limits.
    pub fn can_launch(&self, usage: &NodePoolUsage, cpu_millis: u64, memory_bytes: u64) -> bool {
        if let Some(max) = self.limits.max_nodes {
            if usage.node_count >= max { return false; }
        }
        if let Some(max) = self.limits.max_cpu_millis {
            if usage.cpu_millis + cpu_millis > max { return false; }
        }
        if let Some(max) = self.limits.max_memory_bytes {
            if usage.memory_bytes + memory_bytes > max { return false; }
        }
        true
    }
}
