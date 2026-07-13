//! toml_zoo lane — malformed-scenario robustness.
//!
//! Purpose (docs/costar_microcar_dogfood_plan.md, "Dogfood Lanes > 2. toml_zoo"):
//! feed the `microcar` binary a corpus of malformed scenarios and assert it
//! satisfies the hostile-input contract:
//!
//! * returns a **structured** error (a `microcar: error [<kind>]: ...` line),
//! * **never panics**,
//! * exits with the stable scenario-error code **2** (not a runtime failure and
//!   not a panic/abort),
//! * and reports the **expected error kind**.
//!
//! It also checks **sibling isolation**: a malformed scenario run *concurrently*
//! with a healthy one must not disturb the healthy run (it still passes with its
//! own trace). Because the microcar binary hosts one `World` per process, a
//! "sibling" here is a concurrent sibling process — the property this protects
//! is that one bad input cannot take down a concurrent good run. (In-process
//! multi-session isolation is a later costar-server milestone.)
//!
//! ## Corpus format
//!
//! Each corpus file (`dogfood/toml_zoo/*.toml`) declares the error kind it must
//! produce in a header comment:
//!
//! ```toml
//! # expect-kind: missing-gateway
//! ```
//!
//! Kinds are the stable tags emitted by the microcar binary:
//! * costar structural: `io`, `parse`, `invalid`, `sim`, `trace-mismatch`
//! * microcar semantic: `unknown-firmware`, `missing-gateway`,
//!   `duplicate-bus-node`, `drive-without-powertrain`

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario, RunStatus, ScenarioRun};

/// The exit code the microcar binary uses for a scenario load/validation error.
pub const EXIT_SCENARIO_ERROR: i32 = 2;

/// Default corpus directory, relative to the microcar repo root.
pub const DEFAULT_CORPUS_DIR: &str = "dogfood/toml_zoo";

/// A malformed-scenario case: the file plus the error kind it must produce.
#[derive(Debug, Clone)]
pub struct TomlZooCase {
    pub name: String,
    pub path: PathBuf,
    pub expected_kind: String,
}

/// Outcome of running one case through the microcar binary.
#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: String,
    pub expected_kind: String,
    pub actual_kind: Option<String>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub passed: bool,
    pub detail: String,
}

/// Sibling-isolation outcome: healthy + malformed run concurrently.
#[derive(Debug, Clone)]
pub struct SiblingIsolation {
    pub healthy_scenario: String,
    pub healthy_status: RunStatus,
    pub bad_scenario: String,
    pub bad_status: RunStatus,
    pub bad_exit_code: Option<i32>,
    /// True iff the healthy run passed AND the bad run failed cleanly
    /// (exit 2, not a panic).
    pub isolated: bool,
}

/// The full toml_zoo lane result.
#[derive(Debug, Clone)]
pub struct TomlZooReport {
    pub cases: Vec<CaseResult>,
    pub sibling: Option<SiblingIsolation>,
}

impl TomlZooReport {
    /// (passed, failed) case counts.
    pub fn totals(&self) -> (usize, usize) {
        let passed = self.cases.iter().filter(|c| c.passed).count();
        (passed, self.cases.len() - passed)
    }

    /// True iff every case passed and (if run) sibling isolation held.
    pub fn passed(&self) -> bool {
        !self.cases.is_empty()
            && self.cases.iter().all(|c| c.passed)
            && self.sibling.as_ref().map(|s| s.isolated).unwrap_or(true)
    }

    pub fn to_json(&self) -> Json {
        let cases = self
            .cases
            .iter()
            .map(|c| {
                Json::Obj(vec![
                    ("name".into(), Json::str(&c.name)),
                    ("expected_kind".into(), Json::str(&c.expected_kind)),
                    (
                        "actual_kind".into(),
                        match &c.actual_kind {
                            Some(k) => Json::str(k),
                            None => Json::str(""),
                        },
                    ),
                    ("status".into(), Json::str(c.status.as_str())),
                    (
                        "exit_code".into(),
                        match c.exit_code {
                            Some(code) if code >= 0 => Json::UInt(code as u128),
                            _ => Json::str("none"),
                        },
                    ),
                    ("passed".into(), Json::Bool(c.passed)),
                    ("detail".into(), Json::str(&c.detail)),
                ])
            })
            .collect();
        let (passed, failed) = self.totals();
        let mut obj = vec![
            ("lane".into(), Json::str("toml_zoo")),
            ("passed".into(), Json::Bool(self.passed())),
            ("cases_passed".into(), Json::UInt(passed as u128)),
            ("cases_failed".into(), Json::UInt(failed as u128)),
            ("cases".into(), Json::Arr(cases)),
        ];
        if let Some(s) = &self.sibling {
            obj.push((
                "sibling_isolation".into(),
                Json::Obj(vec![
                    ("healthy_scenario".into(), Json::str(&s.healthy_scenario)),
                    (
                        "healthy_status".into(),
                        Json::str(s.healthy_status.as_str()),
                    ),
                    ("bad_scenario".into(), Json::str(&s.bad_scenario)),
                    ("bad_status".into(), Json::str(s.bad_status.as_str())),
                    ("isolated".into(), Json::Bool(s.isolated)),
                ]),
            ));
        }
        Json::Obj(obj)
    }
}

/// Extract the `# expect-kind:` value from a corpus file's header. Reads the raw
/// bytes (the file may be intentionally invalid TOML), so this must not parse.
pub fn parse_expected_kind(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# expect-kind:") {
            let kind = rest.trim();
            if !kind.is_empty() {
                return Some(kind.to_string());
            }
        }
    }
    None
}

/// Extract the `<kind>` from a `microcar: error [<kind>]: ...` line on stderr.
pub fn parse_error_kind(run: &ScenarioRun) -> Option<String> {
    for line in &run.stderr_tail {
        if let Some((_, after)) = line.split_once("microcar: error [") {
            if let Some((kind, _)) = after.split_once(']') {
                return Some(kind.to_string());
            }
        }
    }
    None
}

/// Discover corpus cases in `dir`: every `*.toml` that carries an
/// `# expect-kind:` header, sorted by path for determinism.
pub fn discover_cases(dir: &Path) -> io::Result<Vec<TomlZooCase>> {
    let mut cases = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(expected_kind) = parse_expected_kind(&path) else {
            continue;
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        cases.push(TomlZooCase {
            name,
            path,
            expected_kind,
        });
    }
    cases.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(cases)
}

/// Evaluate a single case against its run. A case passes iff the binary did NOT
/// panic, exited with the scenario-error code (2), and reported the expected
/// error kind.
pub fn evaluate_case(case: &TomlZooCase, run: &ScenarioRun) -> CaseResult {
    let actual_kind = parse_error_kind(run);
    let no_panic = run.status != RunStatus::Panic;
    let exit_ok = run.exit_code == Some(EXIT_SCENARIO_ERROR);
    let kind_ok = actual_kind.as_deref() == Some(case.expected_kind.as_str());
    let passed = no_panic && exit_ok && kind_ok;

    let detail = if passed {
        format!(
            "structured error [{}], exit {}",
            case.expected_kind, EXIT_SCENARIO_ERROR
        )
    } else if !no_panic {
        format!(
            "PANICKED (status={}, exit={:?}); stderr: {}",
            run.status.as_str(),
            run.exit_code,
            run.stderr_tail.last().cloned().unwrap_or_default()
        )
    } else {
        format!(
            "status={} exit={:?} kind={:?}; expected [{}] with exit {}",
            run.status.as_str(),
            run.exit_code,
            actual_kind,
            case.expected_kind,
            EXIT_SCENARIO_ERROR
        )
    };

    CaseResult {
        name: case.name.clone(),
        expected_kind: case.expected_kind.clone(),
        actual_kind,
        status: run.status,
        exit_code: run.exit_code,
        passed,
        detail,
    }
}

/// Run the whole malformed corpus in `dir` through `bin`, plus (if `healthy` is
/// given and the corpus is non-empty) a sibling-isolation check pairing the
/// healthy scenario with the first corpus case run concurrently.
pub fn run_toml_zoo(
    bin: &Path,
    dir: &Path,
    timeout: Duration,
    healthy: Option<&Path>,
) -> io::Result<TomlZooReport> {
    let cases = discover_cases(dir)?;
    let mut results = Vec::with_capacity(cases.len());
    for case in &cases {
        let run = run_scenario(bin, &case.path, timeout);
        results.push(evaluate_case(case, &run));
    }

    let sibling = match (healthy, cases.first()) {
        (Some(h), Some(bad)) => Some(run_sibling_isolation(bin, h, &bad.path, timeout)),
        _ => None,
    };

    Ok(TomlZooReport {
        cases: results,
        sibling,
    })
}

/// Run a healthy scenario and a malformed scenario concurrently and check that
/// the malformed one did not disturb the healthy one.
pub fn run_sibling_isolation(
    bin: &Path,
    healthy: &Path,
    bad: &Path,
    timeout: Duration,
) -> SiblingIsolation {
    let (bin_a, healthy_a) = (bin.to_path_buf(), healthy.to_path_buf());
    let (bin_b, bad_b) = (bin.to_path_buf(), bad.to_path_buf());

    let h_handle = std::thread::spawn(move || run_scenario(&bin_a, &healthy_a, timeout));
    let b_handle = std::thread::spawn(move || run_scenario(&bin_b, &bad_b, timeout));

    let h = h_handle
        .join()
        .unwrap_or_else(|_| panic!("healthy thread panicked"));
    let b = b_handle
        .join()
        .unwrap_or_else(|_| panic!("bad thread panicked"));

    let isolated = h.status == RunStatus::Pass
        && b.status != RunStatus::Panic
        && b.exit_code == Some(EXIT_SCENARIO_ERROR);

    SiblingIsolation {
        healthy_scenario: h.scenario,
        healthy_status: h.status,
        bad_scenario: b.scenario,
        bad_status: b.status,
        bad_exit_code: b.exit_code,
        isolated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_run(status: RunStatus, exit: Option<i32>, stderr: &[&str]) -> ScenarioRun {
        ScenarioRun {
            scenario: "synthetic".into(),
            status,
            exit_code: exit,
            trace: Vec::new(),
            wall: Duration::from_millis(1),
            stdout_tail: Vec::new(),
            stderr_tail: stderr.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn case(kind: &str) -> TomlZooCase {
        TomlZooCase {
            name: "c".into(),
            path: PathBuf::from("c.toml"),
            expected_kind: kind.to_string(),
        }
    }

    #[test]
    fn parse_error_kind_extracts_tag() {
        let run = synthetic_run(
            RunStatus::Fail,
            Some(2),
            &["microcar: error [missing-gateway]: scenario has a powertrain ECU but no gateway"],
        );
        assert_eq!(parse_error_kind(&run).as_deref(), Some("missing-gateway"));
    }

    #[test]
    fn parse_error_kind_none_when_absent() {
        let run = synthetic_run(RunStatus::Fail, Some(1), &["some other output"]);
        assert_eq!(parse_error_kind(&run), None);
    }

    #[test]
    fn case_passes_on_expected_kind_and_exit_2() {
        let run = synthetic_run(
            RunStatus::Fail,
            Some(2),
            &["microcar: error [parse]: bad toml"],
        );
        let r = evaluate_case(&case("parse"), &run);
        assert!(r.passed, "detail: {}", r.detail);
    }

    #[test]
    fn case_fails_on_wrong_kind() {
        let run = synthetic_run(RunStatus::Fail, Some(2), &["microcar: error [invalid]: x"]);
        let r = evaluate_case(&case("parse"), &run);
        assert!(!r.passed);
    }

    #[test]
    fn case_fails_on_panic_even_if_kind_matches() {
        // A panic is never acceptable, regardless of exit code / stderr text.
        let run = synthetic_run(
            RunStatus::Panic,
            Some(101),
            &["thread 'main' panicked", "microcar: error [parse]: x"],
        );
        let r = evaluate_case(&case("parse"), &run);
        assert!(!r.passed);
        assert!(r.detail.contains("PANICKED"));
    }

    #[test]
    fn case_fails_on_wrong_exit_code() {
        // Right kind + no panic, but exited 1 (runtime) instead of 2 (scenario).
        let run = synthetic_run(RunStatus::Fail, Some(1), &["microcar: error [parse]: x"]);
        let r = evaluate_case(&case("parse"), &run);
        assert!(!r.passed);
    }

    #[test]
    fn parse_expected_kind_reads_header() {
        let dir = std::env::temp_dir().join(format!("tz_hdr_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.toml");
        std::fs::write(&f, "# expect-kind:  duplicate-bus-node\nname = \"x\"\n").unwrap();
        assert_eq!(
            parse_expected_kind(&f).as_deref(),
            Some("duplicate-bus-node")
        );
        let g = dir.join("y.toml");
        std::fs::write(&g, "name = \"y\"\n").unwrap();
        assert_eq!(parse_expected_kind(&g), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_passed_requires_all_cases_and_sibling() {
        let ok = CaseResult {
            name: "a".into(),
            expected_kind: "parse".into(),
            actual_kind: Some("parse".into()),
            status: RunStatus::Fail,
            exit_code: Some(2),
            passed: true,
            detail: String::new(),
        };
        let mut report = TomlZooReport {
            cases: vec![ok.clone()],
            sibling: None,
        };
        assert!(report.passed());
        report.sibling = Some(SiblingIsolation {
            healthy_scenario: "h".into(),
            healthy_status: RunStatus::Pass,
            bad_scenario: "b".into(),
            bad_status: RunStatus::Fail,
            bad_exit_code: Some(2),
            isolated: false,
        });
        assert!(!report.passed(), "sibling non-isolation must fail the lane");
    }
}
