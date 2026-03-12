//! Behavioral conformance test framework with version-gated specs.
//!
//! Provides [`BehaviorSpec`] for defining version-gated behavioral expectations
//! and [`run_conformance`] for executing them against a [`VersionProfile`].

use crate::version::{KarpenterVersion, VersionProfile};
use std::fmt;

/// Version range filter for a behavior spec.
#[derive(Debug, Clone, Default)]
pub struct VersionRange {
    /// Minimum version (inclusive). `None` means no lower bound.
    pub min: Option<KarpenterVersion>,
    /// Maximum version (inclusive). `None` means no upper bound.
    pub max: Option<KarpenterVersion>,
}

impl VersionRange {
    /// Range that matches all versions.
    pub fn all() -> Self {
        Self::default()
    }

    /// Range matching only a single version.
    pub fn exact(v: KarpenterVersion) -> Self {
        Self { min: Some(v), max: Some(v) }
    }

    /// Range starting from `v` (inclusive) with no upper bound.
    pub fn from(v: KarpenterVersion) -> Self {
        Self { min: Some(v), max: None }
    }

    /// Whether `version` falls within this range.
    pub fn contains(&self, version: KarpenterVersion) -> bool {
        let ord = version.ordinal();
        if let Some(min) = self.min {
            if ord < min.ordinal() {
                return false;
            }
        }
        if let Some(max) = self.max {
            if ord > max.ordinal() {
                return false;
            }
        }
        true
    }

    /// Human-readable reason why a version was skipped.
    pub fn skip_reason(&self, version: KarpenterVersion) -> String {
        match (self.min, self.max) {
            (Some(min), Some(max)) if min == max => {
                format!("requires {:?}, running {:?}", min, version)
            }
            (Some(min), Some(max)) => {
                format!("requires {:?}..={:?}, running {:?}", min, max, version)
            }
            (Some(min), None) => {
                format!("requires >= {:?}, running {:?}", min, version)
            }
            (None, Some(max)) => {
                format!("requires <= {:?}, running {:?}", max, version)
            }
            (None, None) => "no version restriction (should not skip)".into(),
        }
    }
}

/// A single behavioral conformance spec.
pub struct BehaviorSpec {
    /// Short identifier (e.g. `"multi-node-consolidation"`).
    pub name: &'static str,
    /// Human-readable description of the expected behavior.
    pub description: &'static str,
    /// Which versions this spec applies to.
    pub applies_to: VersionRange,
    /// Test function: receives a `VersionProfile`, returns `Ok(())` on pass.
    pub test: Box<dyn Fn(&VersionProfile) -> Result<(), String> + Send + Sync>,
}

impl fmt::Debug for BehaviorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BehaviorSpec")
            .field("name", &self.name)
            .field("applies_to", &self.applies_to)
            .finish()
    }
}

/// Outcome of running a single spec.
#[derive(Debug)]
pub enum SpecResult {
    Pass { name: &'static str },
    Fail { name: &'static str, reason: String },
    Skip { name: &'static str, reason: String },
}

/// Summary returned by [`run_conformance`].
#[derive(Debug)]
pub struct ConformanceReport {
    pub results: Vec<SpecResult>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl ConformanceReport {
    /// True if no specs failed.
    pub fn ok(&self) -> bool {
        self.failed == 0
    }
}

impl fmt::Display for ConformanceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.results {
            match r {
                SpecResult::Pass { name } => writeln!(f, "  PASS  {name}")?,
                SpecResult::Fail { name, reason } => writeln!(f, "  FAIL  {name}: {reason}")?,
                SpecResult::Skip { name, reason } => writeln!(f, "  SKIP  {name}: {reason}")?,
            }
        }
        write!(
            f,
            "\n{} passed, {} failed, {} skipped",
            self.passed, self.failed, self.skipped
        )
    }
}

/// Run all `specs` against the given `profile`, returning a summary report.
pub fn run_conformance(profile: &VersionProfile, specs: &[BehaviorSpec]) -> ConformanceReport {
    let mut results = Vec::with_capacity(specs.len());
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);

    for spec in specs {
        if !spec.applies_to.contains(profile.version) {
            let reason = spec.applies_to.skip_reason(profile.version);
            skipped += 1;
            results.push(SpecResult::Skip { name: spec.name, reason });
            continue;
        }
        match (spec.test)(profile) {
            Ok(()) => {
                passed += 1;
                results.push(SpecResult::Pass { name: spec.name });
            }
            Err(reason) => {
                failed += 1;
                results.push(SpecResult::Fail { name: spec.name, reason });
            }
        }
    }

    ConformanceReport { results, passed, failed, skipped }
}
