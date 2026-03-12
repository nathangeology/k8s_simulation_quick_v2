//! Behavioral conformance test framework with version-gated specs.
//!
//! Provides [`BehaviorSpec`] for defining version-specific behavioral expectations
//! and [`ConformanceRunner`] for executing them against a given [`VersionProfile`].

use crate::version::{KarpenterVersion, VersionProfile};
use std::fmt;

/// A version range filter for behavior specs.
#[derive(Debug, Clone)]
pub enum VersionRange {
    /// Applies to all versions.
    All,
    /// Applies only to a specific version.
    Only(KarpenterVersion),
    /// Applies to all versions starting from (inclusive).
    From(KarpenterVersion),
}

impl VersionRange {
    /// Returns true if the given version matches this range.
    pub fn matches(&self, version: KarpenterVersion) -> bool {
        match self {
            Self::All => true,
            Self::Only(v) => *v == version,
            Self::From(v) => version_ord(version) >= version_ord(*v),
        }
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "all versions"),
            Self::Only(v) => write!(f, "{v:?} only"),
            Self::From(v) => write!(f, "{v:?}+"),
        }
    }
}

/// Map version variants to an ordinal for comparison.
fn version_ord(v: KarpenterVersion) -> u32 {
    match v {
        KarpenterVersion::V0_35 => 0,
        KarpenterVersion::V1 => 1,
    }
}

/// Result of running a single behavior spec.
#[derive(Debug)]
pub enum SpecResult {
    Passed,
    Failed(String),
    Skipped(String),
}

/// A behavioral conformance spec.
pub struct BehaviorSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub applies_to: VersionRange,
    pub test_fn: Box<dyn Fn(&VersionProfile) -> Result<(), String>>,
}

impl fmt::Debug for BehaviorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BehaviorSpec")
            .field("name", &self.name)
            .field("applies_to", &self.applies_to)
            .finish()
    }
}

/// Runs conformance specs against a version profile and collects results.
pub struct ConformanceRunner {
    specs: Vec<BehaviorSpec>,
}

/// Summary of a conformance run.
#[derive(Debug, Default)]
pub struct RunSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<(&'static str, SpecResult)>,
}

impl RunSummary {
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

impl ConformanceRunner {
    pub fn new() -> Self {
        Self { specs: Vec::new() }
    }

    pub fn add(&mut self, spec: BehaviorSpec) {
        self.specs.push(spec);
    }

    /// Run all specs against the given profile, printing results to stdout.
    pub fn run(&self, profile: &VersionProfile) -> RunSummary {
        let mut summary = RunSummary::default();

        for spec in &self.specs {
            if !spec.applies_to.matches(profile.version) {
                let reason = format!(
                    "version {:?} outside range: {}",
                    profile.version, spec.applies_to
                );
                println!("  SKIP  {} — {}", spec.name, reason);
                summary.skipped += 1;
                summary.results.push((spec.name, SpecResult::Skipped(reason)));
                continue;
            }

            match (spec.test_fn)(profile) {
                Ok(()) => {
                    println!("  PASS  {}", spec.name);
                    summary.passed += 1;
                    summary.results.push((spec.name, SpecResult::Passed));
                }
                Err(msg) => {
                    println!("  FAIL  {} — {}", spec.name, msg);
                    summary.failed += 1;
                    summary.results.push((spec.name, SpecResult::Failed(msg)));
                }
            }
        }

        println!(
            "\n  {} passed, {} failed, {} skipped",
            summary.passed, summary.failed, summary.skipped
        );
        summary
    }
}

impl Default for ConformanceRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_range_matching() {
        assert!(VersionRange::All.matches(KarpenterVersion::V0_35));
        assert!(VersionRange::All.matches(KarpenterVersion::V1));

        assert!(VersionRange::Only(KarpenterVersion::V1).matches(KarpenterVersion::V1));
        assert!(!VersionRange::Only(KarpenterVersion::V1).matches(KarpenterVersion::V0_35));

        assert!(VersionRange::From(KarpenterVersion::V0_35).matches(KarpenterVersion::V1));
        assert!(!VersionRange::From(KarpenterVersion::V1).matches(KarpenterVersion::V0_35));
    }

    #[test]
    fn runner_skips_out_of_range() {
        let mut runner = ConformanceRunner::new();
        runner.add(BehaviorSpec {
            name: "v1-only-spec",
            description: "only runs on v1",
            applies_to: VersionRange::Only(KarpenterVersion::V1),
            test_fn: Box::new(|_| Ok(())),
        });

        let profile = VersionProfile::new(KarpenterVersion::V0_35);
        let summary = runner.run(&profile);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.passed, 0);
        assert!(summary.all_passed());
    }

    #[test]
    fn runner_reports_pass_and_fail() {
        let mut runner = ConformanceRunner::new();
        runner.add(BehaviorSpec {
            name: "passes",
            description: "always passes",
            applies_to: VersionRange::All,
            test_fn: Box::new(|_| Ok(())),
        });
        runner.add(BehaviorSpec {
            name: "fails",
            description: "always fails",
            applies_to: VersionRange::All,
            test_fn: Box::new(|_| Err("intentional".into())),
        });

        let profile = VersionProfile::new(KarpenterVersion::V1);
        let summary = runner.run(&profile);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert!(!summary.all_passed());
    }
}
