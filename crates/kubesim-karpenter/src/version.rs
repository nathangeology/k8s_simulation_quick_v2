//! Karpenter version abstraction — strategy variants for version-specific behavior.

use serde::{Deserialize, Serialize};

/// Supported Karpenter version profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KarpenterVersion {
    /// v0.35 behavior (pre-v1 GA): Provisioner CRD, simple consolidation,
    /// percentage-only disruption budgets, basic drift detection.
    V0_35,
    /// v1.x behavior (GA): NodePool/NodeClass split, multi-node consolidation,
    /// scheduled disruption budgets, enhanced drift with hash-based detection.
    V1,
}

impl Default for KarpenterVersion {
    fn default() -> Self {
        Self::V1
    }
}

/// Consolidation strategy that varies by Karpenter version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsolidationStrategy {
    /// v0.35: Only remove empty or single underutilized nodes.
    SingleNode,
    /// v1.x: Can replace multiple underutilized nodes with fewer, cheaper ones.
    MultiNode,
}

/// Disruption budget configuration that varies by version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisruptionBudgetConfig {
    /// Max percentage of nodes that may be disrupted (both versions).
    pub max_percent: u32,
    /// v1.x only: specific reasons this budget applies to.
    pub reasons: Vec<DisruptionReason>,
    /// v1.x only: cron schedule window when disruption is allowed.
    pub schedule: Option<String>,
}

impl Default for DisruptionBudgetConfig {
    fn default() -> Self {
        Self {
            max_percent: 10,
            reasons: Vec::new(),
            schedule: None,
        }
    }
}

/// Disruption reasons (v1.x feature — v0.35 treats all disruptions uniformly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisruptionReason {
    Underutilized,
    Empty,
    Drifted,
}

/// Version-resolved behavior profile used by handlers at runtime.
#[derive(Debug, Clone)]
pub struct VersionProfile {
    pub version: KarpenterVersion,
    pub consolidation_strategy: ConsolidationStrategy,
    pub budgets: Vec<DisruptionBudgetConfig>,
    /// v1.x: drift detects NodePool hash changes. v0.35: AMI-only drift.
    pub hash_based_drift: bool,
    /// v1.x: can replace a node with a cheaper instance type during consolidation.
    pub replace_consolidation: bool,
}

impl VersionProfile {
    pub fn new(version: KarpenterVersion) -> Self {
        match version {
            KarpenterVersion::V0_35 => Self {
                version,
                consolidation_strategy: ConsolidationStrategy::SingleNode,
                budgets: vec![DisruptionBudgetConfig::default()],
                hash_based_drift: false,
                replace_consolidation: false,
            },
            KarpenterVersion::V1 => Self {
                version,
                consolidation_strategy: ConsolidationStrategy::MultiNode,
                budgets: vec![DisruptionBudgetConfig::default()],
                hash_based_drift: true,
                replace_consolidation: true,
            },
        }
    }
}
