//! Core type definitions for KubeSim.

use serde::{Deserialize, Serialize};
/// Unique identifier for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Unique identifier for a pod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PodId(pub u32);

/// Unique identifier for an owner (ReplicaSet, Job, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OwnerId(pub u32);

/// Simulation time in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct SimTime(pub u64);

/// Resource quantities (cpu in millicores, memory in bytes).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Resources {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub gpu: u32,
}

/// Pod QoS class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QoSClass {
    Guaranteed,
    Burstable,
    BestEffort,
}

/// Pod lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PodPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Terminating,
}

/// Node lifecycle type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeLifecycle {
    OnDemand,
    Spot { interruption_prob: u32 },
}
