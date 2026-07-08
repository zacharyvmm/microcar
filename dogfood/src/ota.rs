//! ota lane -- over-the-air firmware update happy-path workflow.
//!
//! The lane runs scenarios and checks compact firmware trace events emitted by
//! the gateway OTA dogfood variant. Like the diagnostics and charging lanes it
//! deliberately avoids full golden traces and does not depend on
//! firmware-originated CAN delivery while that simulator path is still being
//! completed.
//!
//! Scenarios declare expectations as `# ota-expect:` comment directives:
//!
//! ```text
//! # ota-expect: state-sequence 0,1,2,3,4,5   (OTA states stepped in order)
//! # ota-expect: crc-ok                        (image CRC verified)
//! # ota-expect: healthy                       (new slot booted healthy)
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario_args, RunStatus};

pub const DEFAULT_OTA_DIR: &str = "dogfood/ota";

const NODE_GATEWAY: u64 = 1;

#[derive(Debug, Clone)]
pub struct OtaCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct OtaScenarioResult {
    pub name: String,
    pub passed: bool,
    pub run_error: Option<String>,
    pub checks: Vec<OtaCheck>,
}

#[derive(Debug, Clone)]
pub struct OtaReport {
    pub scenarios: Vec<OtaScenarioResult>,
}

impl OtaReport {
    pub fn totals(&self) -> (usize, usize) {
        let passed = self.scenarios.iter().filter(|s| s.passed).count();
        (passed, self.scenarios.len() - passed)
    }

    pub fn passed(&self) -> bool {
        !self.scenarios.is_empty() && self.scenarios.iter().all(|s| s.passed)
    }

    pub fn to_json(&self) -> Json {
        let scenarios = self
            .scenarios
            .iter()
            .map(|s| {
                let checks = s
                    .checks
                    .iter()
                    .map(|c| {
                        Json::Obj(vec![
                            ("name".into(), Json::str(&c.name)),
                            ("passed".into(), Json::Bool(c.passed)),
                            ("detail".into(), Json::str(&c.detail)),
                        ])
                    })
                    .collect();
                let mut fields = vec![
                    ("name".into(), Json::str(&s.name)),
                    ("passed".into(), Json::Bool(s.passed)),
                    ("checks".into(), Json::Arr(checks)),
                ];
                if let Some(e) = &s.run_error {
                    fields.push(("run_error".into(), Json::str(e)));
                }
                Json::Obj(fields)
            })
            .collect();
        let (passed, failed) = self.totals();
        Json::Obj(vec![
            ("lane".into(), Json::str("ota")),
            ("passed".into(), Json::Bool(self.passed())),
            ("scenarios_passed".into(), Json::UInt(passed as u128)),
            ("scenarios_failed".into(), Json::UInt(failed as u128)),
            ("scenarios".into(), Json::Arr(scenarios)),
        ])
    }
}

#[derive(Debug, Clone)]
struct UserTrace {
    machine: u64,
    label: String,
    value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expectation {
    StateSequence(Vec<u32>),
    CrcOk,
    Healthy,
}

fn parse_user_trace_line(line: &str) -> Option<UserTrace> {
    let rest = line.trim_start().strip_prefix("[machine.")?;
    let (machine_s, rest) = rest.split_once(']')?;
    let machine = machine_s.parse().ok()?;

    let mut parts = rest.split_whitespace();
    let _at: u64 = parts.next()?.parse().ok()?;
    if parts.next()? != "user-u32" {
        return None;
    }

    let label = parts.next()?.strip_prefix('"')?.strip_suffix('"')?;
    if parts.next()? != "=" {
        return None;
    }
    let value = parts.next()?.parse().ok()?;

    Some(UserTrace {
        machine,
        label: label.to_string(),
        value,
    })
}

/// Ordered list of the `ota_state` values traced by `machine`.
fn state_values(trace: &[UserTrace], machine: u64) -> Vec<u32> {
    trace
        .iter()
        .filter(|e| e.machine == machine && e.label == "ota_state")
        .map(|e| e.value)
        .collect()
}

fn any_value(trace: &[UserTrace], machine: u64, label: &str, want: u32) -> bool {
    trace
        .iter()
        .any(|e| e.machine == machine && e.label == label && e.value == want)
}

/// Whether `want` appears as an ordered (not necessarily contiguous)
/// subsequence of `have`.
fn is_subsequence(have: &[u32], want: &[u32]) -> bool {
    let mut it = have.iter();
    want.iter().all(|w| it.any(|h| h == w))
}

fn parse_expectations(scenario: &Path) -> io::Result<Vec<Expectation>> {
    let content = fs::read_to_string(scenario)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("# ota-expect:") else {
            continue;
        };
        let rest = rest.trim();
        if let Some(args) = rest.strip_prefix("state-sequence ") {
            let seq: Vec<u32> = args
                .trim()
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            out.push(Expectation::StateSequence(seq));
        } else if rest == "crc-ok" {
            out.push(Expectation::CrcOk);
        } else if rest == "healthy" {
            out.push(Expectation::Healthy);
        }
    }
    // Default: a happy-path OTA scenario must at least reach HEALTHY.
    if out.is_empty() {
        out.push(Expectation::Healthy);
    }
    Ok(out)
}

fn evaluate(exp: &Expectation, trace: &[UserTrace]) -> OtaCheck {
    match exp {
        Expectation::StateSequence(want) => {
            let have = state_values(trace, NODE_GATEWAY);
            let passed = !want.is_empty() && is_subsequence(&have, want);
            let detail = if passed {
                format!(
                    "OTA states stepped through the expected sequence {want:?} (traced {have:?})"
                )
            } else {
                format!("expected ota_state subsequence {want:?}; traced {have:?}")
            };
            OtaCheck {
                name: "state-sequence".into(),
                passed,
                detail,
            }
        }
        Expectation::CrcOk => {
            let passed = any_value(trace, NODE_GATEWAY, "ota_crc_ok", 1);
            let detail = if passed {
                "image CRC/signature verified (ota_crc_ok=1)".to_string()
            } else {
                "expected an ota_crc_ok=1 trace event; saw none".to_string()
            };
            OtaCheck {
                name: "crc-ok".into(),
                passed,
                detail,
            }
        }
        Expectation::Healthy => {
            let passed = any_value(trace, NODE_GATEWAY, "ota_boot_result", 1);
            let detail = if passed {
                "new slot booted healthy and update committed (ota_boot_result=1)".to_string()
            } else {
                "expected an ota_boot_result=1 trace event; saw none".to_string()
            };
            OtaCheck {
                name: "healthy".into(),
                passed,
                detail,
            }
        }
    }
}

pub fn discover_scenarios(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

pub fn run_ota_scenario(bin: &Path, scenario: &Path, timeout: Duration) -> OtaScenarioResult {
    let name = scenario
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ota")
        .to_string();
    let run = run_scenario_args(bin, scenario, timeout, &[]);
    if run.status != RunStatus::Pass {
        let detail = run
            .stderr_tail
            .last()
            .cloned()
            .unwrap_or_else(|| format!("run exited {}", run.status.as_str()));
        return OtaScenarioResult {
            name,
            passed: false,
            run_error: Some(detail),
            checks: Vec::new(),
        };
    }

    let trace: Vec<UserTrace> = run
        .trace
        .iter()
        .filter_map(|line| parse_user_trace_line(line))
        .collect();

    let expectations = match parse_expectations(scenario) {
        Ok(e) => e,
        Err(e) => {
            return OtaScenarioResult {
                name,
                passed: false,
                run_error: Some(format!("failed to parse ota expectations: {e}")),
                checks: Vec::new(),
            }
        }
    };
    let checks: Vec<OtaCheck> = expectations
        .iter()
        .map(|exp| evaluate(exp, &trace))
        .collect();
    let passed = !checks.is_empty() && checks.iter().all(|c| c.passed);

    OtaScenarioResult {
        name,
        passed,
        run_error: None,
        checks,
    }
}

pub fn run_ota(bin: &Path, dir: &Path, timeout: Duration) -> io::Result<OtaReport> {
    let scenarios = discover_scenarios(dir)?;
    let results = scenarios
        .iter()
        .map(|s| run_ota_scenario(bin, s, timeout))
        .collect();
    Ok(OtaReport { scenarios: results })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(machine: u64, label: &str, value: u32) -> UserTrace {
        UserTrace {
            machine,
            label: label.into(),
            value,
        }
    }

    fn happy_trace() -> Vec<UserTrace> {
        vec![
            t(NODE_GATEWAY, "ota_state", 0),
            t(NODE_GATEWAY, "ota_state", 1),
            t(NODE_GATEWAY, "ota_state", 2),
            t(NODE_GATEWAY, "ota_crc_ok", 1),
            t(NODE_GATEWAY, "ota_state", 3),
            t(NODE_GATEWAY, "ota_slot", 1),
            t(NODE_GATEWAY, "ota_state", 4),
            t(NODE_GATEWAY, "ota_state", 5),
            t(NODE_GATEWAY, "ota_boot_result", 1),
        ]
    }

    #[test]
    fn parses_user_trace_line() {
        let line = r#"[machine.1]            0 user-u32 "ota_state" = 3"#;
        let event = parse_user_trace_line(line).unwrap();
        assert_eq!(event.machine, NODE_GATEWAY);
        assert_eq!(event.label, "ota_state");
        assert_eq!(event.value, 3);
    }

    #[test]
    fn state_sequence_check_passes_on_full_sequence() {
        let exp = Expectation::StateSequence(vec![0, 1, 2, 3, 4, 5]);
        let c = evaluate(&exp, &happy_trace());
        assert!(c.passed, "{}", c.detail);
    }

    #[test]
    fn state_sequence_check_is_ordered_subsequence() {
        // Extra states interleaved are fine as long as the wanted order holds.
        let trace = vec![
            t(NODE_GATEWAY, "ota_state", 0),
            t(NODE_GATEWAY, "ota_state", 9),
            t(NODE_GATEWAY, "ota_state", 1),
            t(NODE_GATEWAY, "ota_state", 2),
        ];
        assert!(evaluate(&Expectation::StateSequence(vec![0, 1, 2]), &trace).passed);
    }

    #[test]
    fn state_sequence_check_fails_when_out_of_order() {
        // States present but in the wrong order → fail.
        let trace = vec![
            t(NODE_GATEWAY, "ota_state", 2),
            t(NODE_GATEWAY, "ota_state", 1),
            t(NODE_GATEWAY, "ota_state", 0),
        ];
        assert!(!evaluate(&Expectation::StateSequence(vec![0, 1, 2]), &trace).passed);
    }

    #[test]
    fn crc_ok_check_requires_flag() {
        assert!(evaluate(&Expectation::CrcOk, &happy_trace()).passed);

        let without = vec![t(NODE_GATEWAY, "ota_state", 2)];
        assert!(!evaluate(&Expectation::CrcOk, &without).passed);
    }

    #[test]
    fn healthy_check_requires_boot_result() {
        assert!(evaluate(&Expectation::Healthy, &happy_trace()).passed);

        // A failed boot (value 0) does not satisfy healthy.
        let failed = vec![t(NODE_GATEWAY, "ota_boot_result", 0)];
        assert!(!evaluate(&Expectation::Healthy, &failed).passed);
    }

    #[test]
    fn parses_expectations_from_directives() {
        let dir = std::env::temp_dir();
        let path = dir.join("ota_expect_test.toml");
        fs::write(
            &path,
            "# ota-expect: state-sequence 0,1,2,3,4,5\n\
             # ota-expect: crc-ok\n\
             # ota-expect: healthy\n\
             name = \"x\"\n",
        )
        .unwrap();
        let exps = parse_expectations(&path).unwrap();
        assert_eq!(
            exps,
            vec![
                Expectation::StateSequence(vec![0, 1, 2, 3, 4, 5]),
                Expectation::CrcOk,
                Expectation::Healthy,
            ]
        );
        let _ = fs::remove_file(&path);
    }
}
