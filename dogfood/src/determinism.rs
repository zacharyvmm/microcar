//! Solo-vs-repeat determinism check.
//!
//! Runs the same scenario `repeats` times through the microcar binary,
//! normalizes + hashes each trace, and asserts every hash equals the first. This
//! is the foundation the simfarm lane later extends to *concurrent* sessions
//! (the `ConcurrentEqualsSolo` invariant); here we prove the simpler property
//! that repeated *sequential* runs are byte-stable.

use std::path::Path;
use std::time::Duration;

use crate::runner::{run_scenario, RunStatus, ScenarioRun};
use crate::trace_hash::normalized_hash;

/// Outcome of running a scenario `repeats` times.
pub struct DeterminismReport {
    pub scenario: String,
    pub repeats: usize,
    /// One normalized trace hash per run, in order.
    pub hashes: Vec<String>,
    /// True iff there was at least one run and every hash equals the first.
    pub deterministic: bool,
    /// The individual runs (so callers can check invariants without re-running).
    pub runs: Vec<ScenarioRun>,
}

impl DeterminismReport {
    /// The canonical (first) trace hash, if any run happened.
    pub fn trace_hash(&self) -> Option<&str> {
        self.hashes.first().map(|s| s.as_str())
    }

    /// The first run, if any (used to derive per-scenario status/invariants).
    pub fn first_run(&self) -> Option<&ScenarioRun> {
        self.runs.first()
    }

    /// True if any run did not terminate cleanly.
    pub fn any_run_unclean(&self) -> bool {
        self.runs.iter().any(|r| r.status != RunStatus::Pass)
    }
}

/// Run `scenario` `repeats` times and compare normalized trace hashes.
///
/// `repeats` is clamped to a minimum of 1. With `repeats == 1` the report is
/// trivially `deterministic == true` (a single run always equals itself); use
/// `repeats >= 2` for a meaningful determinism assertion.
pub fn check_solo_vs_repeat(
    bin: &Path,
    scenario: &Path,
    repeats: usize,
    timeout: Duration,
) -> DeterminismReport {
    let repeats = repeats.max(1);
    let mut hashes = Vec::with_capacity(repeats);
    let mut runs = Vec::with_capacity(repeats);
    let mut name = scenario.to_string_lossy().into_owned();

    for _ in 0..repeats {
        let run = run_scenario(bin, scenario, timeout);
        name = run.scenario.clone();
        hashes.push(normalized_hash(&run.trace));
        runs.push(run);
    }

    let deterministic = hashes
        .first()
        .map(|first| hashes.iter().all(|h| h == first))
        .unwrap_or(false);

    DeterminismReport {
        scenario: name,
        repeats,
        hashes,
        deterministic,
        runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_equal_hashes_is_deterministic() {
        let hashes = vec!["abc".to_string(); 3];
        let deterministic = hashes.iter().all(|h| h == &hashes[0]);
        assert!(deterministic);
    }

    #[test]
    fn differing_hashes_is_nondeterministic() {
        let hashes = vec!["abc".to_string(), "abc".to_string(), "xyz".to_string()];
        let deterministic = hashes.iter().all(|h| h == &hashes[0]);
        assert!(!deterministic);
    }

    #[test]
    fn report_accessors() {
        let report = DeterminismReport {
            scenario: "s".into(),
            repeats: 2,
            hashes: vec!["h".into(), "h".into()],
            deterministic: true,
            runs: Vec::new(),
        };
        assert_eq!(report.trace_hash(), Some("h"));
        assert!(report.first_run().is_none());
        assert!(!report.any_run_unclean());
    }
}
