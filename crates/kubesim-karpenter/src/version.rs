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

impl KarpenterVersion {
    /// Numeric ordinal for version ordering (used by conformance range checks).
    pub fn ordinal(self) -> u32 {
        match self {
            Self::V0_35 => 0,
            Self::V1 => 1,
        }
    }
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
    /// Budget percentage when schedule is active (e.g. maintenance window). v1.x only.
    pub active_budget: Option<u32>,
    /// Budget percentage when schedule is inactive (e.g. business hours). v1.x only.
    pub inactive_budget: Option<u32>,
}

impl Default for DisruptionBudgetConfig {
    fn default() -> Self {
        Self {
            max_percent: 10,
            reasons: Vec::new(),
            schedule: None,
            active_budget: None,
            inactive_budget: None,
        }
    }
}

/// Evaluate whether a schedule string is currently active at the given SimTime.
///
/// Supported formats:
/// - `"weekday_business_hours"` — Mon–Fri 09:00–17:00
/// - `"maintenance_window"` — Daily 02:00–06:00
/// - `"HH:MM-HH:MM"` — Explicit hour range (e.g. `"02:00-06:00"`)
///
/// SimTime is nanoseconds from epoch. We derive a "sim hour" (0–23) and
/// "sim weekday" (0=Mon..6=Sun) from it using a 24h cycle.
pub fn evaluate_schedule(time: kubesim_core::SimTime, schedule: &str) -> bool {
    const HOUR_NS: u64 = 3_600_000_000_000;
    const DAY_NS: u64 = 24 * HOUR_NS;

    let hour = ((time.0 % DAY_NS) / HOUR_NS) as u32;
    let day = ((time.0 / DAY_NS) % 7) as u32; // 0=Mon..6=Sun

    match schedule {
        "weekday_business_hours" => day < 5 && hour >= 9 && hour < 17,
        "maintenance_window" => hour >= 2 && hour < 6,
        s if s.contains('-') => {
            if let Some((start_s, end_s)) = s.split_once('-') {
                let start = parse_hhmm(start_s);
                let end = parse_hhmm(end_s);
                match (start, end) {
                    (Some(s), Some(e)) => hour >= s && hour < e,
                    _ => false,
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

fn parse_hhmm(s: &str) -> Option<u32> {
    let s = s.trim();
    s.split_once(':').and_then(|(h, _)| h.parse().ok())
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
