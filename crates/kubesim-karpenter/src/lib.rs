//! KubeSim Karpenter — Karpenter provisioner and consolidator model.

pub mod handler;
pub mod nodepool;
pub mod provisioner;
pub mod spot;

pub use kubesim_core;
pub use handler::ProvisioningHandler;
pub use nodepool::{NodePool, NodePoolLimits, NodePoolUsage};
pub use provisioner::{batch_pending_pods, provision, select_instance, PodBatch, ProvisionDecision};
pub use spot::{SpotDisruptionMetrics, SpotInterruptionHandler};
