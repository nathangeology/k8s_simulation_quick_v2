//! KubeSim Karpenter — Karpenter provisioner and consolidator model.

pub mod conformance;
pub mod conformance_consolidation;
pub mod conformance_kwok;
pub mod conformance_provisioning;
pub mod conformance_replicaset;
pub mod conformance_scheduler;
pub mod conformance_version;
pub mod consolidation;
pub mod drift;
pub mod handler;
pub mod nodepool;
pub mod provisioner;
pub mod spot;
pub mod version;

pub use kubesim_core;
pub use conformance::{BehaviorSpec, ConformanceReport, SpecResult, VersionRange, run_conformance};
pub use consolidation::{ConsolidationAction, ConsolidationHandler, ConsolidationPolicy, DrainHandler};
pub use drift::{DriftConfig, DriftHandler};
pub use handler::ProvisioningHandler;
pub use nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
pub use provisioner::{batch_pending_pods, provision, provision_versioned, select_instance, sort_pools_by_weight, PodBatch, ProvisionDecision};
pub use spot::{SpotDisruptionMetrics, SpotInterruptionHandler};
pub use version::{KarpenterVersion, VersionProfile};
