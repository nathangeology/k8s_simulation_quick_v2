//! KubeSim Workload — YAML scenario loader for designed scenarios.
//!
//! Parses study YAML files (scheduling-comparison, deletion-cost, etc.)
//! into typed scenario structs and emits DES events for the simulation engine.

mod scenario;
mod events;
mod loader;

pub use events::Event;
pub use loader::{load_scenario, load_scenario_from_str, variant_events, LoadError};
pub use scenario::*;
