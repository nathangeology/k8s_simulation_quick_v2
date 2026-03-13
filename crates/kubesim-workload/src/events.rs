//! DES events emitted by the workload loader.
//!
//! These events are consumed by the kubesim-engine event loop.

use kubesim_core::{Resources, SimTime};
use serde::{Deserialize, Serialize};

use crate::scenario::{DeletionCostStrategy, ScoringStrategy};

/// A DES event emitted by scenario loading / workload generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// Submit a pod with the given spec at the given time.
    PodSubmitted {
        time: SimTime,
        workload_name: String,
        owner_id: u32,
        requests: Resources,
        limits: Resources,
        priority: i32,
        deletion_cost: Option<i32>,
        duration_ns: Option<u64>,
    },
    /// Launch a node from a node pool.
    NodeLaunching {
        time: SimTime,
        instance_type: String,
        pool_index: u32,
    },
    /// HPA evaluation tick for a workload.
    HpaEvaluation {
        time: SimTime,
        owner_id: u32,
    },
    /// Karpenter provisioning loop tick.
    KarpenterProvisioningLoop {
        time: SimTime,
    },
    /// Karpenter consolidation loop tick.
    KarpenterConsolidationLoop {
        time: SimTime,
    },
    /// Spot interruption check tick (independent of provisioning loop).
    SpotInterruptionCheck {
        time: SimTime,
    },
    /// Traffic level change (for traffic-pattern-driven workloads).
    TrafficChange {
        time: SimTime,
        multiplier: f64,
    },
    /// Metrics snapshot collection.
    MetricsSnapshot {
        time: SimTime,
    },
    /// Configure the scheduler scoring strategy for a variant run.
    ConfigureScheduler {
        scoring: ScoringStrategy,
        weight: i64,
    },
    /// Configure deletion cost strategy for a variant run.
    ConfigureDeletionCost {
        strategy: DeletionCostStrategy,
    },
    /// Submit a ReplicaSet that manages pod lifecycle.
    ReplicaSetSubmitted {
        time: SimTime,
        owner_id: u32,
        desired_replicas: u32,
        requests: Resources,
        limits: Resources,
        priority: i32,
        deletion_cost_strategy: DeletionCostStrategy,
    },
    /// Scale down a ReplicaSet by reducing desired replicas.
    ReplicaSetScaleDown {
        time: SimTime,
        owner_id: u32,
        reduce_by: u32,
    },
    ReplicaSetScaleUp {
        time: SimTime,
        owner_id: u32,
        increase_to: u32,
    },
}

impl Event {
    /// Returns the simulation time for this event (for priority queue ordering).
    pub fn time(&self) -> SimTime {
        match self {
            Event::PodSubmitted { time, .. }
            | Event::NodeLaunching { time, .. }
            | Event::HpaEvaluation { time, .. }
            | Event::KarpenterProvisioningLoop { time }
            | Event::KarpenterConsolidationLoop { time }
            | Event::SpotInterruptionCheck { time }
            | Event::TrafficChange { time, .. }
            | Event::MetricsSnapshot { time } => *time,
            Event::ConfigureScheduler { .. } | Event::ConfigureDeletionCost { .. } => SimTime(0),
            Event::ReplicaSetSubmitted { time, .. }
            | Event::ReplicaSetScaleDown { time, .. } => *time,
            | Event::ReplicaSetScaleUp { time, .. } => *time,
        }
    }
}
