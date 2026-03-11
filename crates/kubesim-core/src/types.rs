//! Core type definitions for KubeSim.

use crate::arena::Handle;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

// ── ID types ────────────────────────────────────────────────────

/// Unique identifier for a node (generational arena handle).
pub type NodeId = Handle<Node>;

/// Unique identifier for a pod (generational arena handle).
pub type PodId = Handle<Pod>;

/// Unique identifier for an owner (ReplicaSet, Job, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OwnerId(pub u32);

/// Simulation time in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct SimTime(pub u64);

// ── Resources ───────────────────────────────────────────────────

/// Resource quantities (cpu in millicores, memory in bytes, gpu count, ephemeral storage in bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resources {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub gpu: u32,
    pub ephemeral_bytes: u64,
}

impl Resources {
    /// Returns true if `self` fits within `capacity` on every dimension.
    pub fn fits_in(&self, capacity: &Resources) -> bool {
        self.cpu_millis <= capacity.cpu_millis
            && self.memory_bytes <= capacity.memory_bytes
            && self.gpu <= capacity.gpu
            && self.ephemeral_bytes <= capacity.ephemeral_bytes
    }

    pub fn saturating_add(&self, other: &Resources) -> Resources {
        Resources {
            cpu_millis: self.cpu_millis.saturating_add(other.cpu_millis),
            memory_bytes: self.memory_bytes.saturating_add(other.memory_bytes),
            gpu: self.gpu.saturating_add(other.gpu),
            ephemeral_bytes: self.ephemeral_bytes.saturating_add(other.ephemeral_bytes),
        }
    }

    pub fn saturating_sub(&self, other: &Resources) -> Resources {
        Resources {
            cpu_millis: self.cpu_millis.saturating_sub(other.cpu_millis),
            memory_bytes: self.memory_bytes.saturating_sub(other.memory_bytes),
            gpu: self.gpu.saturating_sub(other.gpu),
            ephemeral_bytes: self.ephemeral_bytes.saturating_sub(other.ephemeral_bytes),
        }
    }
}

// ── Labels ──────────────────────────────────────────────────────

/// A set of key-value labels (sorted vec for small-set efficiency).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelSet(pub Vec<(String, String)>);

impl LabelSet {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn insert(&mut self, key: String, value: String) {
        if let Some(entry) = self.0.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = value;
        } else {
            self.0.push((key, value));
        }
    }

    pub fn matches(&self, selector: &LabelSelector) -> bool {
        selector.match_labels.0.iter().all(|(k, v)| self.get(k) == Some(v.as_str()))
    }
}

/// Label selector for matching against a [`LabelSet`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelSelector {
    pub match_labels: LabelSet,
}

// ── Taints & Tolerations ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaintEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taint {
    pub key: String,
    pub value: String,
    pub effect: TaintEffect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TolerationOperator {
    Equal,
    Exists,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toleration {
    pub key: String,
    pub operator: TolerationOperator,
    pub value: String,
    pub effect: Option<TaintEffect>,
}

impl Toleration {
    /// Returns true if this toleration matches the given taint.
    pub fn tolerates(&self, taint: &Taint) -> bool {
        if let Some(ref eff) = self.effect {
            if *eff != taint.effect {
                return false;
            }
        }
        match self.operator {
            TolerationOperator::Exists => self.key == taint.key,
            TolerationOperator::Equal => self.key == taint.key && self.value == taint.value,
        }
    }
}

// ── Node types ──────────────────────────────────────────────────

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

/// Node condition flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConditions {
    pub ready: bool,
    pub memory_pressure: bool,
    pub disk_pressure: bool,
    pub pid_pressure: bool,
    pub network_unavailable: bool,
}

// ── Scheduling constraints ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AffinityType {
    Required,
    Preferred { weight: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeAffinityTerm {
    pub affinity_type: AffinityType,
    pub match_labels: LabelSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodAffinityTerm {
    pub affinity_type: AffinityType,
    pub label_selector: LabelSelector,
    pub topology_key: String,
    pub anti: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologySpreadConstraint {
    pub max_skew: u32,
    pub topology_key: String,
    pub when_unsatisfiable: WhenUnsatisfiable,
    pub label_selector: LabelSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhenUnsatisfiable {
    DoNotSchedule,
    ScheduleAnyway,
}

/// All scheduling constraints for a pod.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingConstraints {
    pub node_affinity: Vec<NodeAffinityTerm>,
    pub pod_affinity: Vec<PodAffinityTerm>,
    pub tolerations: Vec<Toleration>,
    pub topology_spread: Vec<TopologySpreadConstraint>,
}

// ── Pod Disruption Budget ───────────────────────────────────────

/// A PodDisruptionBudget constraining voluntary evictions for pods matching a selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodDisruptionBudget {
    pub selector: LabelSelector,
    /// Minimum number of pods that must remain available.
    pub min_available: u32,
}

// ── Node ────────────────────────────────────────────────────────

/// A simulated Kubernetes node.
#[derive(Debug, Clone)]
pub struct Node {
    pub instance_type: String,
    pub allocatable: Resources,
    pub allocated: Resources,
    pub pods: SmallVec<[PodId; 64]>,
    pub conditions: NodeConditions,
    pub labels: LabelSet,
    pub taints: SmallVec<[Taint; 4]>,
    pub cost_per_hour: f64,
    pub lifecycle: NodeLifecycle,
    /// Whether the node is cordoned (unschedulable).
    pub cordoned: bool,
}

// ── Pod ─────────────────────────────────────────────────────────

/// A simulated Kubernetes pod.
#[derive(Debug, Clone)]
pub struct Pod {
    pub requests: Resources,
    pub limits: Resources,
    pub phase: PodPhase,
    pub node: Option<NodeId>,
    pub scheduling_constraints: SchedulingConstraints,
    pub deletion_cost: Option<i32>,
    pub owner: OwnerId,
    pub qos_class: QoSClass,
    pub priority: i32,
    pub labels: LabelSet,
}
