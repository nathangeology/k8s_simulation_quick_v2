//! KubeSim Karpenter — Karpenter provisioner and consolidator model.

pub mod drift;
pub mod handler;
pub mod nodepool;
pub mod provisioner;

pub use kubesim_core;
pub use drift::{DriftConfig, DriftHandler};
pub use handler::ProvisioningHandler;
pub use nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
pub use provisioner::{batch_pending_pods, provision, select_instance, PodBatch, ProvisionDecision};
