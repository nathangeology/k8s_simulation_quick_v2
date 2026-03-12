//! Behavioral conformance test framework with version-gated specs.
//!
//! Defines a `BehaviorSpec` struct and a runner that iterates specs,
//! skips those outside the current version range, and runs applicable ones.

use crate::version::KarpenterVersion;

/// A behavioral conformance spec: name, description, version range, and test function.
pub struct BehaviorSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// Karpenter versions this spec applies to.
    pub applies_to: &'static [KarpenterVersion],
    /// The test function. Returns Ok(()) on pass, Err(reason) on failure.
    pub test_fn: fn() -> Result<(), String>,
}

/// Result of running a single spec.
#[derive(Debug)]
pub enum SpecResult {
    Pass,
    Fail(String),
    Skipped(String),
}

/// Run all specs against the given version, returning results.
pub fn run_specs(specs: &[BehaviorSpec], version: KarpenterVersion) -> Vec<(&'static str, SpecResult)> {
    specs
        .iter()
        .map(|spec| {
            if !spec.applies_to.contains(&version) {
                return (spec.name, SpecResult::Skipped(
                    format!("not applicable to {:?}", version),
                ));
            }
            match (spec.test_fn)() {
                Ok(()) => (spec.name, SpecResult::Pass),
                Err(reason) => (spec.name, SpecResult::Fail(reason)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_inapplicable_version() {
        let specs = [BehaviorSpec {
            name: "v1-only",
            description: "test",
            applies_to: &[KarpenterVersion::V1],
            test_fn: || Ok(()),
        }];
        let results = run_specs(&specs, KarpenterVersion::V0_35);
        assert!(matches!(results[0].1, SpecResult::Skipped(_)));
    }

    #[test]
    fn runs_applicable_spec() {
        let specs = [BehaviorSpec {
            name: "both",
            description: "test",
            applies_to: &[KarpenterVersion::V0_35, KarpenterVersion::V1],
            test_fn: || Ok(()),
        }];
        let results = run_specs(&specs, KarpenterVersion::V1);
        assert!(matches!(results[0].1, SpecResult::Pass));
    }

    #[test]
    fn captures_failure() {
        let specs = [BehaviorSpec {
            name: "fail",
            description: "test",
            applies_to: &[KarpenterVersion::V1],
            test_fn: || Err("broken".into()),
        }];
        let results = run_specs(&specs, KarpenterVersion::V1);
        assert!(matches!(results[0].1, SpecResult::Fail(_)));
    }
}
