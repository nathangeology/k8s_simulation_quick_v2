//! KubeSim Workload — YAML scenario loader, trace replay, and random generation.
//!
//! Parses study YAML files (scheduling-comparison, deletion-cost, etc.)
//! into typed scenario structs and emits DES events for the simulation engine.
//! Also supports trace replay from Prometheus CSV and Kubernetes event log exports,
//! and random scenario generation with workload archetypes.

mod scenario;
mod events;
mod loader;
mod trace;
mod random;

pub use events::Event;
pub use loader::{load_scenario, load_scenario_from_str, load_scenario_from_str_seeded, variant_events, LoadError};
pub use scenario::*;
pub use trace::{load_trace, load_trace_from_str, TraceFormat};
pub use random::{
    generate_random_scenario, RandomScenarioConfig, RangeU32,
    InstanceWeight, ArchetypeWeights,
};
