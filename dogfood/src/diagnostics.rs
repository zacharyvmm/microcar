//! diagnostics lane -- service-mode and DTC query/clear workflow.
//!
//! The lane runs scenarios and checks compact firmware trace events emitted by
//! the gateway and powertrain. This deliberately avoids full golden traces and
//! avoids depending on firmware-originated CAN delivery while that simulator
//! path is still being completed.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario_args, RunStatus};

pub const DEFAULT_DIAGNOSTICS_DIR: &str = "dogfood/diagnostics";

const NODE_GATEWAY: u64 = 1;
const NODE_POWERTRAIN: u64 = 2;

const DIAG_START_SESSION: u8 = 1;
const DIAG_READ_MODE: u8 = 2;
const DIAG_READ_DTCS: u8 = 3;
const DIAG_ACTUATOR_TEST: u8 = 6;

const DIAG_OK: u8 = 0;
const VEHICLE_SERVICE: u8 = 6;

#[derive(Debug, Clone)]
pub struct DiagnosticsCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsScenarioResult {
    pub name: String,
    pub passed: bool,
    pub run_error: Option<String>,
    pub checks: Vec<DiagnosticsCheck>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsReport {
    pub scenarios: Vec<DiagnosticsScenarioResult>,
}

impl DiagnosticsReport {
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
            ("lane".into(), Json::str("diagnostics")),
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

#[derive(Debug, Clone)]
struct DiagResponse {
    service: u8,
    req: u8,
    status: u8,
    value0: u8,
    value1: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expectation {
    ServiceMode,
    Dtc {
        req: u8,
        count: u8,
        code: Option<u8>,
    },
    Actuator {
        status: u8,
    },
    MotorTorque {
        max: i8,
        after_ms: u64,
    },
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

fn decode_response_head(value: u32) -> DiagResponse {
    DiagResponse {
        req: ((value >> 24) & 0xff) as u8,
        service: ((value >> 16) & 0xff) as u8,
        status: ((value >> 8) & 0xff) as u8,
        value0: (value & 0xff) as u8,
        value1: 0,
    }
}

fn decode_response_v1(value: u32) -> ((u8, u8), u8) {
    let req = ((value >> 24) & 0xff) as u8;
    let service = ((value >> 16) & 0xff) as u8;
    ((service, req), (value & 0xff) as u8)
}

fn collect_diag_responses(trace: &[UserTrace]) -> Vec<DiagResponse> {
    let mut responses = BTreeMap::<(u8, u8), DiagResponse>::new();

    for event in trace.iter().filter(|e| e.machine == NODE_GATEWAY) {
        match event.label.as_str() {
            "gateway_diag_response" => {
                let response = decode_response_head(event.value);
                responses.insert((response.service, response.req), response);
            }
            "gateway_diag_response_v1" => {
                let (key, value1) = decode_response_v1(event.value);
                responses
                    .entry(key)
                    .or_insert(DiagResponse {
                        service: key.0,
                        req: key.1,
                        status: 0,
                        value0: 0,
                        value1: 0,
                    })
                    .value1 = value1;
            }
            _ => {}
        }
    }

    responses.into_values().collect()
}

fn response<'a>(responses: &'a [DiagResponse], service: u8, req: u8) -> Option<&'a DiagResponse> {
    responses
        .iter()
        .find(|r| r.service == service && r.req == req)
}

fn collect_service_motor_commands(trace: &[UserTrace]) -> Vec<i8> {
    trace
        .iter()
        .filter(|e| e.machine == NODE_POWERTRAIN && e.label == "diag_motor_command")
        .map(|e| ((e.value >> 8) as u8) as i8)
        .collect()
}

fn parse_expectations(scenario: &Path) -> io::Result<Vec<Expectation>> {
    let content = fs::read_to_string(scenario)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("# diagnostics-expect:") else {
            continue;
        };
        let rest = rest.trim();
        if rest == "service-mode" {
            out.push(Expectation::ServiceMode);
        } else if let Some(args) = rest.strip_prefix("dtc ") {
            let req = parse_kv_u8(args, "req").unwrap_or(3);
            let count = parse_kv_u8(args, "count").unwrap_or(0);
            let code = parse_kv_u8(args, "code");
            out.push(Expectation::Dtc { req, count, code });
        } else if let Some(args) = rest.strip_prefix("actuator ") {
            let status = match parse_kv_str(args, "status").unwrap_or("ok") {
                "ok" => DIAG_OK,
                "rejected" => 1,
                _ => DIAG_OK,
            };
            out.push(Expectation::Actuator { status });
        } else if let Some(args) = rest.strip_prefix("motor-torque ") {
            let max = parse_kv_i8(args, "max").unwrap_or(0);
            let after_ms = parse_kv_u64(args, "after_ms").unwrap_or(0);
            out.push(Expectation::MotorTorque { max, after_ms });
        }
    }
    if out.is_empty() {
        out.push(Expectation::ServiceMode);
    }
    Ok(out)
}

fn parse_kv_str<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    s.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
}

fn parse_kv_u8(s: &str, key: &str) -> Option<u8> {
    parse_kv_str(s, key)?.parse().ok()
}

fn parse_kv_i8(s: &str, key: &str) -> Option<i8> {
    parse_kv_str(s, key)?.parse().ok()
}

fn parse_kv_u64(s: &str, key: &str) -> Option<u64> {
    parse_kv_str(s, key)?.parse().ok()
}

fn evaluate(
    exp: &Expectation,
    responses: &[DiagResponse],
    service_motor_commands: &[i8],
) -> DiagnosticsCheck {
    match exp {
        Expectation::ServiceMode => {
            let start = response(responses, DIAG_START_SESSION, 1);
            let mode = response(responses, DIAG_READ_MODE, 2);
            let passed = start.is_some_and(|r| r.status == DIAG_OK && r.value0 == VEHICLE_SERVICE)
                && mode.is_some_and(|r| r.status == DIAG_OK && r.value0 == VEHICLE_SERVICE);
            let detail = if passed {
                "gateway entered SERVICE and read-mode returned SERVICE".to_string()
            } else {
                format!(
                    "expected START_SESSION and READ_MODE status=0 mode=6; start={:?} mode={:?}",
                    start.map(response_payload),
                    mode.map(response_payload)
                )
            };
            DiagnosticsCheck {
                name: "service-mode".into(),
                passed,
                detail,
            }
        }
        Expectation::Dtc { req, count, code } => {
            let resp = response(responses, DIAG_READ_DTCS, *req);
            let passed = resp.is_some_and(|r| {
                r.status == DIAG_OK && r.value0 == *count && code.is_none_or(|c| r.value1 == c)
            });
            let detail = if passed {
                match code {
                    Some(c) => format!("request {req} returned {count} DTC(s), first code {c}"),
                    None => format!("request {req} returned {count} DTC(s)"),
                }
            } else {
                format!(
                    "expected READ_DTCS req={req} count={count} code={code:?}; got {:?}",
                    resp.map(response_payload)
                )
            };
            DiagnosticsCheck {
                name: format!("dtc-req-{req}"),
                passed,
                detail,
            }
        }
        Expectation::Actuator { status } => {
            let resp = response(responses, DIAG_ACTUATOR_TEST, 6);
            let passed = resp.is_some_and(|r| r.status == *status);
            let detail = if passed {
                format!("actuator self-test returned status {status}")
            } else {
                format!(
                    "expected ACTUATOR_TEST status {status}; got {:?}",
                    resp.map(response_payload)
                )
            };
            DiagnosticsCheck {
                name: "actuator-test".into(),
                passed,
                detail,
            }
        }
        Expectation::MotorTorque { max, after_ms } => {
            let passed = !service_motor_commands.is_empty()
                && service_motor_commands.iter().all(|t| *t <= *max);
            let detail = if passed {
                format!(
                    "all {} SERVICE motor command(s) were <= {max} (expectation after {after_ms}ms)",
                    service_motor_commands.len()
                )
            } else {
                format!("expected SERVICE motor torque <= {max}; saw {service_motor_commands:?}")
            };
            DiagnosticsCheck {
                name: "motor-torque".into(),
                passed,
                detail,
            }
        }
    }
}

fn response_payload(r: &DiagResponse) -> Vec<u8> {
    vec![1, r.service, r.req, r.status, r.value0, r.value1]
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

pub fn run_diagnostics_scenario(
    bin: &Path,
    scenario: &Path,
    timeout: Duration,
) -> DiagnosticsScenarioResult {
    let name = scenario
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("diagnostics")
        .to_string();
    let run = run_scenario_args(bin, scenario, timeout, &[]);
    if run.status != RunStatus::Pass {
        let detail = run
            .stderr_tail
            .last()
            .cloned()
            .unwrap_or_else(|| format!("run exited {}", run.status.as_str()));
        return DiagnosticsScenarioResult {
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
    let responses = collect_diag_responses(&trace);
    let service_motor_commands = collect_service_motor_commands(&trace);

    let expectations = match parse_expectations(scenario) {
        Ok(e) => e,
        Err(e) => {
            return DiagnosticsScenarioResult {
                name,
                passed: false,
                run_error: Some(format!("failed to parse diagnostics expectations: {e}")),
                checks: Vec::new(),
            }
        }
    };
    let checks: Vec<DiagnosticsCheck> = expectations
        .iter()
        .map(|exp| evaluate(exp, &responses, &service_motor_commands))
        .collect();
    let passed = !checks.is_empty() && checks.iter().all(|c| c.passed);

    DiagnosticsScenarioResult {
        name,
        passed,
        run_error: None,
        checks,
    }
}

pub fn run_diagnostics(bin: &Path, dir: &Path, timeout: Duration) -> io::Result<DiagnosticsReport> {
    let scenarios = discover_scenarios(dir)?;
    let results = scenarios
        .iter()
        .map(|s| run_diagnostics_scenario(bin, s, timeout))
        .collect();
    Ok(DiagnosticsReport { scenarios: results })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_trace_line() {
        let line = r#"[machine.1]            0 user-u32 "gateway_diag_response" = 16842758"#;
        let event = parse_user_trace_line(line).unwrap();
        assert_eq!(event.machine, NODE_GATEWAY);
        assert_eq!(event.label, "gateway_diag_response");
        assert_eq!(event.value, 16842758);
    }

    #[test]
    fn collects_diag_response_pair() {
        let trace = vec![
            UserTrace {
                machine: NODE_GATEWAY,
                label: "gateway_diag_response".into(),
                value: (3 << 24) | ((DIAG_READ_DTCS as u32) << 16) | 1,
            },
            UserTrace {
                machine: NODE_GATEWAY,
                label: "gateway_diag_response_v1".into(),
                value: (3 << 24) | ((DIAG_READ_DTCS as u32) << 16) | 7,
            },
        ];
        let responses = collect_diag_responses(&trace);
        let dtc = response(&responses, DIAG_READ_DTCS, 3).unwrap();
        assert_eq!(dtc.status, DIAG_OK);
        assert_eq!(dtc.value0, 1);
        assert_eq!(dtc.value1, 7);
    }

    #[test]
    fn collects_service_motor_commands() {
        let trace = vec![UserTrace {
            machine: NODE_POWERTRAIN,
            label: "diag_motor_command".into(),
            value: 0,
        }];
        assert_eq!(collect_service_motor_commands(&trace), vec![0]);
    }
}
