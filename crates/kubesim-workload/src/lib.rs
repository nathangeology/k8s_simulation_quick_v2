//! KubeSim Workload — YAML scenario loader and trace replay for designed scenarios.
//!
//! Parses study YAML files (scheduling-comparison, deletion-cost, etc.)
//! into typed scenario structs and emits DES events for the simulation engine.
//! Also supports trace replay from Prometheus CSV and Kubernetes event log exports.

mod scenario;
mod events;
mod loader;
mod trace;

pub use events::Event;
pub use loader::{load_scenario, load_scenario_from_str, variant_events, LoadError};
pub use scenario::*;
pub use trace::{load_trace, load_trace_from_str, TraceFormat};
