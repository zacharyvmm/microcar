//! charging lane -- EV charging safety workflow.
//!
//! The lane runs scenarios and checks compact firmware trace events emitted by
//! the gateway and powertrain charging dogfood variants. Like the diagnostics
//! lane it deliberately avoids full golden traces and does not depend on
//! firmware-originated CAN delivery while that simulator path is still being
//! completed.
//!
//! Scenarios declare expectations as `# charging-expect:` comment directives:
//!
//! ```text
//! # charging-expect: charging-mode        (gateway entered CHARGING on plug-in)
//! # charging-expect: drive-blocked        (a drive request while plugged stayed CHARGING)
//! # charging-expect: motor-torque max=0   (powertrain clamped torque <= 0, motor disabled)
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario_args, RunStatus};

pub const DEFAULT_CHARGING_DIR: &str = "dogfood/charging";

const NODE_GATEWAY: u64 = 1;
const NODE_POWERTRAIN: u64 = 2;

/// Vehicle mode value for CHARGING (mirrors `VEHICLE_CHARGING` in
/// `common/include/microcar_protocol.h`).
const VEHICLE_CHARGING: u32 = 5;

#[derive(Debug, Clone)]
pub struct ChargingCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct ChargingScenarioResult {
    pub name: String,
    pub passed: bool,
    pub run_error: Option<String>,
    pub checks: Vec<ChargingCheck>,
}

#[derive(Debug, Clone)]
pub struct ChargingReport {
    pub scenarios: Vec<ChargingScenarioResult>,
}

impl ChargingReport {
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
            ("lane".into(), Json::str("charging")),
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
    ChargingMode,
    DriveBlocked,
    MotorTorque { max: i8 },
}

/// A single decoded `charging_motor_command` trace: `(torque, motor_enable)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MotorCommand {
    torque: i8,
    enable: u8,
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

/// Latest value of `label` traced by `machine`, if any.
fn last_value(trace: &[UserTrace], machine: u64, label: &str) -> Option<u32> {
    trace
        .iter()
        .rfind(|e| e.machine == machine && e.label == label)
        .map(|e| e.value)
}

fn any_value(trace: &[UserTrace], machine: u64, label: &str, want: u32) -> bool {
    trace
        .iter()
        .any(|e| e.machine == machine && e.label == label && e.value == want)
}

fn motor_commands(trace: &[UserTrace]) -> Vec<MotorCommand> {
    trace
        .iter()
        .filter(|e| e.machine == NODE_POWERTRAIN && e.label == "charging_motor_command")
        .map(|e| MotorCommand {
            torque: ((e.value >> 8) as u8) as i8,
            enable: (e.value & 0xff) as u8,
        })
        .collect()
}

fn parse_expectations(scenario: &Path) -> io::Result<Vec<Expectation>> {
    let content = fs::read_to_string(scenario)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("# charging-expect:") else {
            continue;
        };
        let rest = rest.trim();
        if rest == "charging-mode" {
            out.push(Expectation::ChargingMode);
        } else if rest == "drive-blocked" {
            out.push(Expectation::DriveBlocked);
        } else if let Some(args) = rest.strip_prefix("motor-torque ") {
            let max = parse_kv_i8(args, "max").unwrap_or(0);
            out.push(Expectation::MotorTorque { max });
        }
    }
    // Default: a charging scenario must at least reach CHARGING.
    if out.is_empty() {
        out.push(Expectation::ChargingMode);
    }
    Ok(out)
}

fn parse_kv_i8(s: &str, key: &str) -> Option<i8> {
    let prefix = format!("{key}=");
    s.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))?
        .parse()
        .ok()
}

fn evaluate(exp: &Expectation, trace: &[UserTrace]) -> ChargingCheck {
    match exp {
        Expectation::ChargingMode => {
            let passed = any_value(
                trace,
                NODE_GATEWAY,
                "gateway_charging_state",
                VEHICLE_CHARGING,
            );
            let detail = if passed {
                "gateway entered CHARGING after plug-in".to_string()
            } else {
                format!(
                    "expected a gateway_charging_state == {VEHICLE_CHARGING}; last was {:?}",
                    last_value(trace, NODE_GATEWAY, "gateway_charging_state")
                )
            };
            ChargingCheck {
                name: "charging-mode".into(),
                passed,
                detail,
            }
        }
        Expectation::DriveBlocked => {
            // The gateway traces charging_drive_blocked=1 when a drive request
            // while plugged left the vehicle in CHARGING.
            let blocked = last_value(trace, NODE_GATEWAY, "charging_drive_blocked");
            let still_charging =
                last_value(trace, NODE_GATEWAY, "gateway_charging_state") == Some(VEHICLE_CHARGING);
            let passed = blocked == Some(1) && still_charging;
            let detail = if passed {
                "drive request while plugged was refused; vehicle stayed in CHARGING".to_string()
            } else {
                format!(
                    "expected charging_drive_blocked=1 and mode still CHARGING; \
                     got blocked={blocked:?} still_charging={still_charging}"
                )
            };
            ChargingCheck {
                name: "drive-blocked".into(),
                passed,
                detail,
            }
        }
        Expectation::MotorTorque { max } => {
            let cmds = motor_commands(trace);
            let passed = !cmds.is_empty() && cmds.iter().all(|c| c.torque <= *max && c.enable == 0);
            let detail = if passed {
                format!(
                    "all {} CHARGING motor command(s) clamped torque <= {max} with motor disabled",
                    cmds.len()
                )
            } else if cmds.is_empty() {
                "expected charging_motor_command trace events; saw none".to_string()
            } else {
                format!(
                    "expected CHARGING torque <= {max} and motor disabled; saw {:?}",
                    cmds.iter()
                        .map(|c| (c.torque, c.enable))
                        .collect::<Vec<_>>()
                )
            };
            ChargingCheck {
                name: "motor-torque".into(),
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

pub fn run_charging_scenario(
    bin: &Path,
    scenario: &Path,
    timeout: Duration,
) -> ChargingScenarioResult {
    let name = scenario
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("charging")
        .to_string();
    let run = run_scenario_args(bin, scenario, timeout, &[]);
    if run.status != RunStatus::Pass {
        let detail = run
            .stderr_tail
            .last()
            .cloned()
            .unwrap_or_else(|| format!("run exited {}", run.status.as_str()));
        return ChargingScenarioResult {
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
            return ChargingScenarioResult {
                name,
                passed: false,
                run_error: Some(format!("failed to parse charging expectations: {e}")),
                checks: Vec::new(),
            }
        }
    };
    let checks: Vec<ChargingCheck> = expectations
        .iter()
        .map(|exp| evaluate(exp, &trace))
        .collect();
    let passed = !checks.is_empty() && checks.iter().all(|c| c.passed);

    ChargingScenarioResult {
        name,
        passed,
        run_error: None,
        checks,
    }
}

pub fn run_charging(bin: &Path, dir: &Path, timeout: Duration) -> io::Result<ChargingReport> {
    let scenarios = discover_scenarios(dir)?;
    let results = scenarios
        .iter()
        .map(|s| run_charging_scenario(bin, s, timeout))
        .collect();
    Ok(ChargingReport { scenarios: results })
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

    #[test]
    fn parses_user_trace_line() {
        let line = r#"[machine.1]            0 user-u32 "gateway_charging_state" = 5"#;
        let event = parse_user_trace_line(line).unwrap();
        assert_eq!(event.machine, NODE_GATEWAY);
        assert_eq!(event.label, "gateway_charging_state");
        assert_eq!(event.value, 5);
    }

    #[test]
    fn charging_mode_check_passes_on_charging_state() {
        let trace = vec![t(NODE_GATEWAY, "gateway_charging_state", VEHICLE_CHARGING)];
        let c = evaluate(&Expectation::ChargingMode, &trace);
        assert!(c.passed, "{}", c.detail);
    }

    #[test]
    fn charging_mode_check_fails_without_charging_state() {
        let trace = vec![t(NODE_GATEWAY, "vehicle_mode", 1)];
        let c = evaluate(&Expectation::ChargingMode, &trace);
        assert!(!c.passed);
    }

    #[test]
    fn drive_blocked_check_requires_flag_and_charging() {
        let ok = vec![
            t(NODE_GATEWAY, "charging_drive_blocked", 1),
            t(NODE_GATEWAY, "gateway_charging_state", VEHICLE_CHARGING),
        ];
        assert!(evaluate(&Expectation::DriveBlocked, &ok).passed);

        // Blocked flag set but somehow not in CHARGING → fail.
        let bad = vec![
            t(NODE_GATEWAY, "charging_drive_blocked", 0),
            t(NODE_GATEWAY, "gateway_charging_state", VEHICLE_CHARGING),
        ];
        assert!(!evaluate(&Expectation::DriveBlocked, &bad).passed);
    }

    #[test]
    fn motor_torque_check_requires_zero_and_disabled() {
        // torque 0, enable 0 → pass.
        let ok = vec![t(NODE_POWERTRAIN, "charging_motor_command", 0)];
        assert!(evaluate(&Expectation::MotorTorque { max: 0 }, &ok).passed);

        // torque 40, enable 1 → fail.
        let bad = vec![t(
            NODE_POWERTRAIN,
            "charging_motor_command",
            (40u32 << 8) | 1,
        )];
        assert!(!evaluate(&Expectation::MotorTorque { max: 0 }, &bad).passed);

        // no motor commands at all → fail.
        assert!(!evaluate(&Expectation::MotorTorque { max: 0 }, &[]).passed);
    }

    #[test]
    fn parses_expectations_from_directives() {
        let dir = std::env::temp_dir();
        let path = dir.join("charging_expect_test.toml");
        fs::write(
            &path,
            "# charging-expect: charging-mode\n\
             # charging-expect: drive-blocked\n\
             # charging-expect: motor-torque max=0\n\
             name = \"x\"\n",
        )
        .unwrap();
        let exps = parse_expectations(&path).unwrap();
        assert_eq!(
            exps,
            vec![
                Expectation::ChargingMode,
                Expectation::DriveBlocked,
                Expectation::MotorTorque { max: 0 },
            ]
        );
        let _ = fs::remove_file(&path);
    }
}
