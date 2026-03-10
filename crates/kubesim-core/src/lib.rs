//! KubeSim Core — ClusterState, Node, Pod, and Resources data structures.

pub mod arena;
pub mod state;
pub mod types;

pub use arena::{Arena, Handle};
pub use state::ClusterState;
pub use types::*;
