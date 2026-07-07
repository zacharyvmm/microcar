//! Serializable run summary for CI.
//!
//! Aggregates per-scenario results (status, wall time, trace hash, determinism,
//! invariant results) plus totals, and writes them as pretty JSON via the
//! hand-rolled [`crate::json`] emitter.

use std::fs;
use std::io;
use std::path::Path;

use crate::determinism::DeterminismReport;
use crate::invariants::{check_default, CheckStatus, InvariantResult};
use crate::json::Json;
use crate::runner::RunStatus;

/// Harness version, from Cargo.
pub const HARNESS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-invariant entry in the summary.
#[derive(Debug, Clone)]
pub struct InvariantEntry {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

impl From<&InvariantResult> for InvariantEntry {
    fn from(r: &InvariantResult) -> Self {
        Self {
            name: r.name.clone(),
            status: r.status,
            message: r.message.clone(),
        }
    }
}

/// One scenario's rolled-up result.
#[derive(Debug, Clone)]
pub struct ScenarioSummary {
    pub name: String,
    pub status: RunStatus,
    pub wall_ms: u128,
    pub trace_hash: String,
    pub repeats: usize,
    pub deterministic: bool,
    pub repeat_hashes: Vec<String>,
    pub invariants: Vec<InvariantEntry>,
}

impl ScenarioSummary {
    /// Build a scenario summary from a determinism report, checking the default
    /// invariant set against the report's first run.
    pub fn from_determinism(report: &DeterminismReport) -> Self {
        let (status, wall_ms, invariants) = match report.first_run() {
            Some(run) => {
                let inv = check_default(run)
                    .iter()
                    .map(InvariantEntry::from)
                    .collect::<Vec<_>>();
                (run.status, run.wall_ms(), inv)
            }
            None => (RunStatus::Fail, 0, Vec::new()),
        };
        ScenarioSummary {
            name: report.scenario.clone(),
            status,
            wall_ms,
            trace_hash: report.trace_hash().unwrap_or("").to_string(),
            repeats: report.repeats,
            deterministic: report.deterministic,
            repeat_hashes: report.hashes.clone(),
            invariants,
        }
    }

    /// A scenario "passes" iff it terminated cleanly, was deterministic, and no
    /// invariant hard-failed.
    pub fn passed(&self) -> bool {
        self.status == RunStatus::Pass
            && self.deterministic
            && !self.invariants.iter().any(|i| i.status == CheckStatus::Fail)
    }

    fn to_json(&self) -> Json {
        let invariants = self
            .invariants
            .iter()
            .map(|i| {
                Json::Obj(vec![
                    ("name".into(), Json::str(&i.name)),
                    ("status".into(), Json::str(i.status.as_str())),
                    ("message".into(), Json::str(&i.message)),
                ])
            })
            .collect();
        let repeat_hashes = self.repeat_hashes.iter().map(Json::str).collect();
        Json::Obj(vec![
            ("name".into(), Json::str(&self.name)),
            ("status".into(), Json::str(self.status.as_str())),
            ("passed".into(), Json::Bool(self.passed())),
            ("wall_ms".into(), Json::UInt(self.wall_ms)),
            ("trace_hash".into(), Json::str(&self.trace_hash)),
            ("repeats".into(), Json::UInt(self.repeats as u128)),
            ("deterministic".into(), Json::Bool(self.deterministic)),
            ("repeat_hashes".into(), Json::Arr(repeat_hashes)),
            ("invariants".into(), Json::Arr(invariants)),
        ])
    }
}

/// Totals across all scenarios.
#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub scenarios: usize,
    pub passed: usize,
    pub failed: usize,
    pub invariants_passed: usize,
    pub invariants_failed: usize,
    pub invariants_skipped: usize,
}

/// The complete harness summary.
#[derive(Debug, Clone)]
pub struct Summary {
    pub harness_version: String,
    pub scenarios: Vec<ScenarioSummary>,
    pub totals: Totals,
}

impl Summary {
    pub fn new(scenarios: Vec<ScenarioSummary>) -> Self {
        let mut totals = Totals {
            scenarios: scenarios.len(),
            ..Default::default()
        };
        for s in &scenarios {
            if s.passed() {
                totals.passed += 1;
            } else {
                totals.failed += 1;
            }
            for inv in &s.invariants {
                match inv.status {
                    CheckStatus::Pass => totals.invariants_passed += 1,
                    CheckStatus::Fail => totals.invariants_failed += 1,
                    CheckStatus::Skipped => totals.invariants_skipped += 1,
                }
            }
        }
        Summary {
            harness_version: HARNESS_VERSION.to_string(),
            scenarios,
            totals,
        }
    }

    /// True if every scenario passed.
    pub fn all_passed(&self) -> bool {
        self.totals.failed == 0 && self.totals.scenarios > 0
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("harness_version".into(), Json::str(&self.harness_version)),
            (
                "scenarios".into(),
                Json::Arr(self.scenarios.iter().map(ScenarioSummary::to_json).collect()),
            ),
            (
                "totals".into(),
                Json::Obj(vec![
                    ("scenarios".into(), Json::UInt(self.totals.scenarios as u128)),
                    ("passed".into(), Json::UInt(self.totals.passed as u128)),
                    ("failed".into(), Json::UInt(self.totals.failed as u128)),
                    (
                        "invariants_passed".into(),
                        Json::UInt(self.totals.invariants_passed as u128),
                    ),
                    (
                        "invariants_failed".into(),
                        Json::UInt(self.totals.invariants_failed as u128),
                    ),
                    (
                        "invariants_skipped".into(),
                        Json::UInt(self.totals.invariants_skipped as u128),
                    ),
                ]),
            ),
        ])
    }

    pub fn to_json_string(&self) -> String {
        self.to_json().to_pretty()
    }
}

/// Write a JSON summary built from the given determinism reports to `path`.
pub fn write_summary(reports: &[DeterminismReport], path: &Path) -> io::Result<()> {
    let summary = build_summary(reports);
    fs::write(path, summary.to_json_string())
}

/// Build a [`Summary`] from determinism reports (checks invariants per scenario).
pub fn build_summary(reports: &[DeterminismReport]) -> Summary {
    Summary::new(reports.iter().map(ScenarioSummary::from_determinism).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::ScenarioRun;
    use std::time::Duration;

    fn fake_run(status: RunStatus, trace: &[&str]) -> ScenarioRun {
        ScenarioRun {
            scenario: "demo".into(),
            status,
            exit_code: Some(0),
            trace: trace.iter().map(|s| s.to_string()).collect(),
            wall: Duration::from_millis(42),
            stdout_tail: Vec::new(),
            stderr_tail: Vec::new(),
        }
    }

    fn passing_report() -> DeterminismReport {
        DeterminismReport {
            scenario: "demo".into(),
            repeats: 2,
            hashes: vec!["deadbeef".into(), "deadbeef".into()],
            deterministic: true,
            runs: vec![
                fake_run(RunStatus::Pass, &["[machine.1] 10 x", "[machine.1] 20 x"]),
                fake_run(RunStatus::Pass, &["[machine.1] 10 x", "[machine.1] 20 x"]),
            ],
        }
    }

    #[test]
    fn passing_scenario_summarizes_as_passed() {
        let s = ScenarioSummary::from_determinism(&passing_report());
        assert!(s.passed());
        assert_eq!(s.trace_hash, "deadbeef");
        assert_eq!(s.wall_ms, 42);
        // implemented invariants present.
        assert!(s
            .invariants
            .iter()
            .any(|i| i.name == "VirtualTimeMonotonic" && i.status == CheckStatus::Pass));
    }

    #[test]
    fn nondeterministic_scenario_fails() {
        let mut report = passing_report();
        report.deterministic = false;
        report.hashes = vec!["a".into(), "b".into()];
        let s = ScenarioSummary::from_determinism(&report);
        assert!(!s.passed());
    }

    #[test]
    fn timeout_scenario_fails_invariant() {
        let report = DeterminismReport {
            scenario: "slow".into(),
            repeats: 1,
            hashes: vec!["h".into()],
            deterministic: true,
            runs: vec![fake_run(RunStatus::Timeout, &["[machine.1] 10 x"])],
        };
        let s = ScenarioSummary::from_determinism(&report);
        assert!(!s.passed());
        assert!(s
            .invariants
            .iter()
            .any(|i| i.name == "ProcessTerminatedCleanly" && i.status == CheckStatus::Fail));
    }

    #[test]
    fn summary_json_is_wellformed_and_has_fields() {
        let summary = build_summary(&[passing_report()]);
        assert!(summary.all_passed());
        let json = summary.to_json_string();
        assert!(json.contains("\"harness_version\""));
        assert!(json.contains("\"trace_hash\": \"deadbeef\""));
        assert!(json.contains("\"deterministic\": true"));
        assert!(json.contains("\"totals\""));
        // Balanced braces/brackets => structurally valid.
        assert_eq!(json.matches('{').count(), json.matches('}').count());
        assert_eq!(json.matches('[').count(), json.matches(']').count());
    }

    #[test]
    fn totals_count_invariants() {
        let summary = build_summary(&[passing_report()]);
        // 3 implemented (pass) + 7 stubs (skipped) per scenario.
        assert_eq!(summary.totals.invariants_passed, 3);
        assert_eq!(summary.totals.invariants_skipped, 7);
        assert_eq!(summary.totals.invariants_failed, 0);
    }
}
