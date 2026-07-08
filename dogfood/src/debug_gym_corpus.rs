//! debug_gym seeded-bug corpus — paired buggy/fixed firmware seeds.
//!
//! Purpose (docs/costar_microcar_dogfood_plan.md, "Dogfood Lanes > 5.
//! debug_gym"): make microcar an AI-debugging benchmark. The determinism
//! *primitives* the corpus relies on are already exercised by the `debug-gym`
//! lane (M7/M8/M10/M11: `step`, `continue_until`, keyframe replay, message
//! breakpoint). This module adds the *seeded-bug corpus* itself: each seed is a
//! deliberately-buggy firmware variant paired with the fixed firmware, and the
//! plan's required metadata — description, expected symptom, minimal failing
//! scenario, golden failing trace, required debugging primitive, and fixed
//! trace.
//!
//! Each seed runs both its **failing** (buggy firmware) and **fixed** scenario
//! through the product binary and asserts three things:
//!
//! * **bug-reproduced** — the buggy firmware exhibits the documented symptom
//!   (the golden *failing* trace),
//! * **bug-fixed** — the fixed firmware behaves correctly (the *fixed* trace),
//! * **traces-diverge** — the two traces differ at the documented point, which
//!   is exactly what the seed's debugging primitive localizes.
//!
//! A seed is only "genuinely exercised" (not fabricated): the failing trace is
//! produced by a real, opt-in buggy firmware variant, and the fixed trace by
//! the real correct firmware. The corpus is green when every bug reproduces in
//! its buggy firmware and is absent in its fixed firmware.
//!
//! First seed — **ota_rollback**: a broken CRC check accepts a corrupt OTA
//! image, so the update commits and boots the bad slot (`firmware/
//! gateway_ota_crcbug`) instead of rolling back (`firmware/gateway_ota_badcrc`).

use std::io;
use std::path::Path;
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario_args, RunStatus, ScenarioRun};

pub const DEFAULT_CORPUS_DIR: &str = "dogfood/debug_gym";

/// The gateway is machine id 1 in every corpus scenario.
const NODE_GATEWAY: u64 = 1;
/// The powertrain is machine id 2 in every corpus scenario.
const NODE_POWERTRAIN: u64 = 2;

/// Diagnostic service id for a "read stored DTCs" request.
const DIAG_READ_DTCS: u8 = 3;
/// Request id the clear-DTCs script uses for the READ_DTCS *before* the clear.
const REQ_READ_DTCS_PRE: u8 = 3;
/// Request id the clear-DTCs script uses for the READ_DTCS *after* the clear.
const REQ_READ_DTCS_POST: u8 = 5;

/// Diagnostic service id for a "start diagnostic session" request.
const DIAG_START_SESSION: u8 = 1;
/// Diagnostic response status: request accepted.
const DIAG_OK: u8 = 0;
/// Diagnostic response status: request rejected.
const DIAG_REJECTED: u8 = 1;
/// Vehicle mode: DRIVE.
const VEHICLE_DRIVE: u8 = 2;
/// Vehicle mode: SERVICE.
const VEHICLE_SERVICE: u8 = 6;

/// A user-u32 firmware trace event: `[machine.N] <t> user-u32 "label" = value`.
#[derive(Debug, Clone)]
struct UserTrace {
    machine: u64,
    label: String,
    value: u32,
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

fn parse_traces(trace: &[String]) -> Vec<UserTrace> {
    trace
        .iter()
        .filter_map(|l| parse_user_trace_line(l))
        .collect()
}

fn any_value(trace: &[UserTrace], machine: u64, label: &str, want: u32) -> bool {
    trace
        .iter()
        .any(|e| e.machine == machine && e.label == label && e.value == want)
}

fn last_value(trace: &[UserTrace], machine: u64, label: &str) -> Option<u32> {
    trace
        .iter()
        .filter(|e| e.machine == machine && e.label == label)
        .map(|e| e.value)
        .next_back()
}

/// Ordered `ota_state` values traced by `machine`.
fn state_values(trace: &[UserTrace], machine: u64) -> Vec<u32> {
    trace
        .iter()
        .filter(|e| e.machine == machine && e.label == "ota_state")
        .map(|e| e.value)
        .collect()
}

/// A SERVICE-mode powertrain motor command, decoded from the packed
/// `diag_motor_command` trace value (`(torque << 8) | motor_enable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MotorCommand {
    torque: i8,
    motor_enable: bool,
}

fn service_motor_commands(trace: &[UserTrace]) -> Vec<MotorCommand> {
    trace
        .iter()
        .filter(|e| e.machine == NODE_POWERTRAIN && e.label == "diag_motor_command")
        .map(|e| MotorCommand {
            torque: ((e.value >> 8) & 0xff) as u8 as i8,
            motor_enable: (e.value & 0x1) == 1,
        })
        .collect()
}

/// Decode `value0` of a `gateway_diag_response` for the given service + request.
///
/// The gateway packs the response head as
/// `(request_id << 24) | (service << 16) | (status << 8) | value0`.
fn diag_response_value0(trace: &[UserTrace], service: u8, req: u8) -> Option<u8> {
    trace
        .iter()
        .filter(|e| e.machine == NODE_GATEWAY && e.label == "gateway_diag_response")
        .map(|e| e.value)
        .find(|v| ((v >> 24) & 0xff) as u8 == req && ((v >> 16) & 0xff) as u8 == service)
        .map(|v| (v & 0xff) as u8)
}

/// The DTC count reported by a READ_DTCS response with the given request id.
fn dtc_count(trace: &[UserTrace], req: u8) -> Option<u8> {
    diag_response_value0(trace, DIAG_READ_DTCS, req)
}

/// Decode `(status, value0)` of a `gateway_diag_response` for the given
/// service + request (`(req<<24)|(service<<16)|(status<<8)|value0`).
fn diag_response_status_value0(trace: &[UserTrace], service: u8, req: u8) -> Option<(u8, u8)> {
    trace
        .iter()
        .filter(|e| e.machine == NODE_GATEWAY && e.label == "gateway_diag_response")
        .map(|e| e.value)
        .find(|v| ((v >> 24) & 0xff) as u8 == req && ((v >> 16) & 0xff) as u8 == service)
        .map(|v| (((v >> 8) & 0xff) as u8, (v & 0xff) as u8))
}

/// Which built-in bug a seed represents (selects the assertion logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedKind {
    /// OTA rollback bug: a broken CRC check accepts a corrupt image, so the
    /// update boots the bad slot instead of rolling back.
    OtaRollback,
    /// SERVICE-mode torque clamp bug: the powertrain skips the SERVICE safety
    /// clamp, so a service session still commands drive torque.
    ServiceTorqueClamp,
    /// Clear-all-DTCs bug: a BMS-scoped CLEAR_DTCS wrongly clears every node's
    /// DTCs, silently dropping an unrelated (powertrain) fault.
    ClearAllDtcs,
    /// START_SESSION-in-DRIVE bug: the gateway skips the safety guard that
    /// refuses a diagnostic session while driving, so a service session opens
    /// mid-drive.
    StartSessionInDrive,
}

/// A seeded-bug corpus entry: metadata + the buggy/fixed scenario pair.
#[derive(Debug, Clone)]
pub struct Seed {
    pub name: &'static str,
    pub kind: SeedKind,
    /// What the bug is.
    pub description: &'static str,
    /// The observable symptom in the failing trace.
    pub symptom: &'static str,
    /// The debugging primitive that localizes the bug.
    pub primitive: &'static str,
    /// Scenario running the buggy firmware (relative to the corpus dir).
    pub failing_scenario: &'static str,
    /// Scenario running the fixed firmware (relative to the corpus dir).
    pub fixed_scenario: &'static str,
}

/// The built-in seeded-bug corpus.
pub fn builtin_seeds() -> Vec<Seed> {
    vec![
        Seed {
            name: "ota_rollback",
            kind: SeedKind::OtaRollback,
            description: "A broken OTA CRC check accepts a corrupt image as valid, \
                      so the slot model commits and boots the bad slot instead \
                      of rolling back to the known-good slot.",
            symptom: "A corrupt OTA image reaches HEALTHY (ota_boot_result=1) with \
                  no ota_rollback — the vehicle boots unverified firmware.",
            primitive: "state trace / continue_until(ota_state): step to VERIFYING \
                    and inspect ota_crc_ok — the buggy firmware reports \
                    ota_crc_ok=1 on a corrupt image where the fix reports 0, so \
                    the runs diverge into HEALTHY vs ROLLED_BACK.",
            failing_scenario: "ota_rollback_bug/failing.toml",
            fixed_scenario: "ota_rollback_bug/fixed.toml",
        },
        Seed {
            name: "service_torque",
            kind: SeedKind::ServiceTorqueClamp,
            description: "The powertrain skips the SERVICE-mode safety clamp, so a \
                      diagnostic service session still commands drive torque \
                      with the motor enabled.",
            symptom: "During a SERVICE session an 80% throttle demand produces a \
                  diag_motor_command with torque>0 and motor_enable=1 — the \
                  drivetrain is live while the vehicle is being serviced.",
            primitive: "message breakpoint / continue_until on the \
                    diag_motor_command trace: stop at the SERVICE-mode motor \
                    command and inspect the torque + motor_enable — the buggy \
                    firmware commands torque>0/enabled where the fix commands \
                    0/disabled.",
            failing_scenario: "service_torque_bug/failing.toml",
            fixed_scenario: "service_torque_bug/fixed.toml",
        },
        Seed {
            name: "clear_all_dtcs",
            kind: SeedKind::ClearAllDtcs,
            description: "A diagnostic CLEAR_DTCS scoped to the BMS wrongly clears \
                      EVERY node's DTCs (fault_manager_clear_all instead of \
                      fault_manager_clear_node), silently dropping an unrelated \
                      powertrain fault that was never serviced.",
            symptom: "After a BMS-scoped clear, a follow-up READ_DTCS reports 0 \
                  DTCs — the powertrain fault has vanished (the fix leaves 1 \
                  behind).",
            primitive: "message breakpoint / continue_until on the \
                    gateway_diag_response trace: stop at the post-clear READ_DTCS \
                    response and inspect its count — the buggy firmware reports 0 \
                    where the fix reports 1, so the runs diverge at the clear.",
            failing_scenario: "clear_dtcs_bug/failing.toml",
            fixed_scenario: "clear_dtcs_bug/fixed.toml",
        },
        Seed {
            name: "start_session_in_drive",
            kind: SeedKind::StartSessionInDrive,
            description: "The gateway skips the safety guard that refuses a \
                      diagnostic START_SESSION while the vehicle is in DRIVE, so \
                      a service session opens mid-drive and the vehicle is \
                      commanded out of DRIVE into SERVICE while moving.",
            symptom: "A START_SESSION request sent while in DRIVE is accepted \
                  (gateway_diag_response status=OK, mode=SERVICE) instead of \
                  rejected — the drivetrain state machine leaves DRIVE mid-drive.",
            primitive: "message breakpoint / continue_until on the \
                    gateway_diag_response trace: stop at the START_SESSION \
                    response and inspect its status + mode — the buggy firmware \
                    reports OK/SERVICE where the fix reports REJECTED/DRIVE.",
            failing_scenario: "start_session_drive_bug/failing.toml",
            fixed_scenario: "start_session_drive_bug/fixed.toml",
        },
    ]
}

/// One assertion within a seed.
#[derive(Debug, Clone)]
pub struct CorpusCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// The result of running one seed (its failing + fixed scenarios).
#[derive(Debug, Clone)]
pub struct SeedResult {
    pub name: String,
    pub description: String,
    pub symptom: String,
    pub primitive: String,
    pub passed: bool,
    pub run_error: Option<String>,
    pub checks: Vec<CorpusCheck>,
}

/// The whole corpus result.
#[derive(Debug, Clone)]
pub struct CorpusReport {
    pub seeds: Vec<SeedResult>,
}

impl CorpusReport {
    pub fn totals(&self) -> (usize, usize) {
        let passed = self.seeds.iter().filter(|s| s.passed).count();
        (passed, self.seeds.len() - passed)
    }

    pub fn passed(&self) -> bool {
        !self.seeds.is_empty() && self.seeds.iter().all(|s| s.passed)
    }

    pub fn to_json(&self) -> Json {
        let seeds = self
            .seeds
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
                    ("description".into(), Json::str(&s.description)),
                    ("symptom".into(), Json::str(&s.symptom)),
                    ("primitive".into(), Json::str(&s.primitive)),
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
            ("lane".into(), Json::str("debug_gym_corpus")),
            ("passed".into(), Json::Bool(self.passed())),
            ("seeds_passed".into(), Json::UInt(passed as u128)),
            ("seeds_failed".into(), Json::UInt(failed as u128)),
            ("seeds".into(), Json::Arr(seeds)),
        ])
    }
}

/// Parsed traces for a seed's paired runs.
struct SeedTraces {
    failing: Vec<UserTrace>,
    fixed: Vec<UserTrace>,
}

/// Evaluate a seed's assertions from its failing and fixed traces.
fn evaluate(kind: SeedKind, traces: &SeedTraces) -> Vec<CorpusCheck> {
    match kind {
        SeedKind::OtaRollback => evaluate_ota_rollback(traces),
        SeedKind::ServiceTorqueClamp => evaluate_service_torque_clamp(traces),
        SeedKind::ClearAllDtcs => evaluate_clear_all_dtcs(traces),
        SeedKind::StartSessionInDrive => evaluate_start_session_in_drive(traces),
    }
}

fn evaluate_start_session_in_drive(traces: &SeedTraces) -> Vec<CorpusCheck> {
    // The START_SESSION response (service=1, req=1): (status, value0=mode).
    let f = diag_response_status_value0(&traces.failing, DIAG_START_SESSION, 1);
    let x = diag_response_status_value0(&traces.fixed, DIAG_START_SESSION, 1);

    // bug-reproduced: the buggy firmware ACCEPTS the mid-drive START_SESSION
    // (status OK) and opens a SERVICE session.
    let bug_reproduced = f == Some((DIAG_OK, VEHICLE_SERVICE));
    let reproduced_detail = if bug_reproduced {
        "buggy firmware accepted START_SESSION mid-drive (status=OK, mode=SERVICE) — the \
         DRIVE guard was skipped"
            .to_string()
    } else {
        format!(
            "expected the buggy firmware to accept START_SESSION mid-drive (status=OK, \
             mode=SERVICE); got {f:?}"
        )
    };

    // bug-fixed: the correct firmware REJECTS the request and stays in DRIVE.
    let bug_fixed = x == Some((DIAG_REJECTED, VEHICLE_DRIVE));
    let fixed_detail = if bug_fixed {
        "fixed firmware rejected START_SESSION mid-drive (status=REJECTED, mode=DRIVE) — the \
         vehicle stayed in DRIVE"
            .to_string()
    } else {
        format!(
            "expected the fixed firmware to reject START_SESSION mid-drive (status=REJECTED, \
             mode=DRIVE); got {x:?}"
        )
    };

    // traces-diverge: the START_SESSION status is the localizing signal — OK
    // (buggy, session opened) vs REJECTED (fixed, session refused).
    let f_status = f.map(|(s, _)| s);
    let x_status = x.map(|(s, _)| s);
    let diverge = f_status == Some(DIAG_OK) && x_status == Some(DIAG_REJECTED);
    let diverge_detail = if diverge {
        "START_SESSION status diverges: OK/accepted (buggy) vs REJECTED (fixed)".to_string()
    } else {
        format!(
            "expected START_SESSION status OK (buggy) vs REJECTED (fixed); got {f_status:?} vs \
             {x_status:?}"
        )
    };

    vec![
        CorpusCheck {
            name: "bug-reproduced".into(),
            passed: bug_reproduced,
            detail: reproduced_detail,
        },
        CorpusCheck {
            name: "bug-fixed".into(),
            passed: bug_fixed,
            detail: fixed_detail,
        },
        CorpusCheck {
            name: "traces-diverge".into(),
            passed: diverge,
            detail: diverge_detail,
        },
    ]
}

fn evaluate_clear_all_dtcs(traces: &SeedTraces) -> Vec<CorpusCheck> {
    let f = &traces.failing;
    let x = &traces.fixed;

    let f_pre = dtc_count(f, REQ_READ_DTCS_PRE);
    let f_post = dtc_count(f, REQ_READ_DTCS_POST);
    let x_pre = dtc_count(x, REQ_READ_DTCS_PRE);
    let x_post = dtc_count(x, REQ_READ_DTCS_POST);

    // bug-reproduced: both DTCs are present before the clear (count 2), but the
    // BMS-scoped clear wrongly drops the powertrain DTC too (post-clear 0).
    let bug_reproduced = f_pre == Some(2) && f_post == Some(0);
    let reproduced_detail = if bug_reproduced {
        "buggy firmware cleared ALL DTCs on a BMS-scoped clear: 2 before, 0 after \
         (the powertrain fault was silently dropped)"
            .to_string()
    } else {
        format!(
            "expected the buggy firmware to go 2 → 0 DTCs on a BMS-scoped clear; \
             got pre={f_pre:?} post={f_post:?}"
        )
    };

    // bug-fixed: both DTCs present before the clear (count 2), and only the BMS
    // DTC is cleared — the powertrain DTC survives (post-clear 1).
    let bug_fixed = x_pre == Some(2) && x_post == Some(1);
    let fixed_detail = if bug_fixed {
        "fixed firmware cleared only BMS DTCs: 2 before, 1 after (the powertrain \
         fault survived the BMS-scoped clear)"
            .to_string()
    } else {
        format!(
            "expected the fixed firmware to go 2 → 1 DTCs on a BMS-scoped clear; \
             got pre={x_pre:?} post={x_post:?}"
        )
    };

    // traces-diverge: the post-clear DTC count is the localizing signal — 0
    // (buggy, everything cleared) vs 1 (fixed, only BMS cleared).
    let diverge = f_post == Some(0) && x_post == Some(1);
    let diverge_detail = if diverge {
        "post-clear READ_DTCS count diverges: 0 (buggy) vs 1 (fixed)".to_string()
    } else {
        format!("expected post-clear count 0 (buggy) vs 1 (fixed); got {f_post:?} vs {x_post:?}")
    };

    vec![
        CorpusCheck {
            name: "bug-reproduced".into(),
            passed: bug_reproduced,
            detail: reproduced_detail,
        },
        CorpusCheck {
            name: "bug-fixed".into(),
            passed: bug_fixed,
            detail: fixed_detail,
        },
        CorpusCheck {
            name: "traces-diverge".into(),
            passed: diverge,
            detail: diverge_detail,
        },
    ]
}

fn evaluate_service_torque_clamp(traces: &SeedTraces) -> Vec<CorpusCheck> {
    let f = service_motor_commands(&traces.failing);
    let x = service_motor_commands(&traces.fixed);

    // bug-reproduced: the buggy firmware commands drive torque during a SERVICE
    // session (some motor command with torque>0 and the motor enabled).
    let bug_reproduced = !f.is_empty() && f.iter().any(|c| c.torque > 0 && c.motor_enable);
    let f_max = f.iter().map(|c| c.torque).max().unwrap_or(0);
    let reproduced_detail = if bug_reproduced {
        format!(
            "buggy firmware commanded drive torque in SERVICE (max torque {f_max}, motor enabled) \
             — the SERVICE clamp was skipped"
        )
    } else if f.is_empty() {
        "expected SERVICE-mode diag_motor_command events from the buggy firmware; saw none"
            .to_string()
    } else {
        format!(
            "expected the buggy firmware to command torque>0 in SERVICE; max torque was {f_max}"
        )
    };

    // bug-fixed: the correct firmware clamps every SERVICE motor command to
    // torque 0 with the motor disabled.
    let bug_fixed = !x.is_empty() && x.iter().all(|c| c.torque == 0 && !c.motor_enable);
    let x_max = x.iter().map(|c| c.torque).max().unwrap_or(0);
    let fixed_detail = if bug_fixed {
        "fixed firmware clamped every SERVICE motor command to torque 0 with the motor disabled"
            .to_string()
    } else if x.is_empty() {
        "expected SERVICE-mode diag_motor_command events from the fixed firmware; saw none"
            .to_string()
    } else {
        format!("expected the fixed firmware to clamp SERVICE torque to 0; max torque was {x_max}")
    };

    // traces-diverge: the buggy run commands positive torque where the fixed run
    // commands zero — the localizing signal on the diag_motor_command trace.
    let diverge = f_max > 0 && x_max == 0;
    let diverge_detail = if diverge {
        format!(
            "SERVICE motor commands diverge: buggy max torque {f_max} (motor enabled) vs fixed 0 \
             (motor disabled)"
        )
    } else {
        format!(
            "expected divergence (buggy torque>0 vs fixed 0); got buggy {f_max} vs fixed {x_max}"
        )
    };

    vec![
        CorpusCheck {
            name: "bug-reproduced".into(),
            passed: bug_reproduced,
            detail: reproduced_detail,
        },
        CorpusCheck {
            name: "bug-fixed".into(),
            passed: bug_fixed,
            detail: fixed_detail,
        },
        CorpusCheck {
            name: "traces-diverge".into(),
            passed: diverge,
            detail: diverge_detail,
        },
    ]
}

fn evaluate_ota_rollback(traces: &SeedTraces) -> Vec<CorpusCheck> {
    let f = &traces.failing;
    let x = &traces.fixed;

    // bug-reproduced: the buggy firmware boots the corrupt image (HEALTHY,
    // boot_result=1) and never rolls back.
    let booted = any_value(f, NODE_GATEWAY, "ota_boot_result", 1);
    let no_rollback = !any_value(f, NODE_GATEWAY, "ota_rollback", 1);
    let bug_reproduced = booted && no_rollback;
    let reproduced_detail = if bug_reproduced {
        "buggy firmware booted the corrupt image (ota_boot_result=1, no ota_rollback)".to_string()
    } else if !booted {
        "expected the buggy firmware to reach a healthy boot (ota_boot_result=1); it did not"
            .to_string()
    } else {
        "expected the buggy firmware NOT to roll back, but it emitted ota_rollback=1".to_string()
    };

    // bug-fixed: the correct firmware rolls back to slot A and never boots the
    // corrupt image.
    let rolled_back = any_value(x, NODE_GATEWAY, "ota_rollback", 1);
    let slot_a = last_value(x, NODE_GATEWAY, "ota_active_slot") == Some(0);
    let never_booted = !any_value(x, NODE_GATEWAY, "ota_boot_result", 1);
    let bug_fixed = rolled_back && slot_a && never_booted;
    let fixed_detail = if bug_fixed {
        "fixed firmware rolled back to slot A (ota_rollback=1, ota_active_slot=0, never booted)"
            .to_string()
    } else if !rolled_back {
        "expected the fixed firmware to roll back (ota_rollback=1); it did not".to_string()
    } else if !slot_a {
        "expected the fixed firmware to stay on slot A (ota_active_slot=0)".to_string()
    } else {
        "expected the fixed firmware to never boot the corrupt image, but ota_boot_result=1"
            .to_string()
    };

    // traces-diverge: the localizing signal. The buggy CRC check reports the
    // corrupt image valid (crc_ok=1) where the fix reports it bad (crc_ok=0),
    // and the runs terminate in HEALTHY(5) vs ROLLED_BACK(6).
    let f_crc = last_value(f, NODE_GATEWAY, "ota_crc_ok");
    let x_crc = last_value(x, NODE_GATEWAY, "ota_crc_ok");
    let crc_diverges = f_crc == Some(1) && x_crc == Some(0);
    let f_end = state_values(f, NODE_GATEWAY).last().copied();
    let x_end = state_values(x, NODE_GATEWAY).last().copied();
    let state_diverges = f_end == Some(5) && x_end == Some(6);
    let diverge = crc_diverges && state_diverges;
    let diverge_detail = if diverge {
        "traces diverge at VERIFYING: ota_crc_ok 1 (buggy) vs 0 (fixed), ending HEALTHY(5) vs \
         ROLLED_BACK(6)"
            .to_string()
    } else {
        format!(
            "expected divergence crc_ok 1 vs 0 and end-state 5 vs 6; got crc {f_crc:?} vs {x_crc:?}, \
             end {f_end:?} vs {x_end:?}"
        )
    };

    vec![
        CorpusCheck {
            name: "bug-reproduced".into(),
            passed: bug_reproduced,
            detail: reproduced_detail,
        },
        CorpusCheck {
            name: "bug-fixed".into(),
            passed: bug_fixed,
            detail: fixed_detail,
        },
        CorpusCheck {
            name: "traces-diverge".into(),
            passed: diverge,
            detail: diverge_detail,
        },
    ]
}

fn run_error(run: &ScenarioRun, which: &str) -> String {
    let tail = run
        .stderr_tail
        .last()
        .cloned()
        .unwrap_or_else(|| format!("exited {}", run.status.as_str()));
    format!("{which} scenario run failed: {tail}")
}

/// Run one seed: its failing (buggy) and fixed scenarios.
pub fn run_seed(bin: &Path, dir: &Path, seed: &Seed, timeout: Duration) -> SeedResult {
    let failing_path = dir.join(seed.failing_scenario);
    let fixed_path = dir.join(seed.fixed_scenario);

    let base = |run_error: Option<String>, checks: Vec<CorpusCheck>, passed: bool| SeedResult {
        name: seed.name.to_string(),
        description: seed.description.to_string(),
        symptom: seed.symptom.to_string(),
        primitive: seed.primitive.to_string(),
        passed,
        run_error,
        checks,
    };

    let failing = run_scenario_args(bin, &failing_path, timeout, &[]);
    if failing.status != RunStatus::Pass {
        return base(Some(run_error(&failing, "failing")), Vec::new(), false);
    }
    let fixed = run_scenario_args(bin, &fixed_path, timeout, &[]);
    if fixed.status != RunStatus::Pass {
        return base(Some(run_error(&fixed, "fixed")), Vec::new(), false);
    }

    let traces = SeedTraces {
        failing: parse_traces(&failing.trace),
        fixed: parse_traces(&fixed.trace),
    };
    let checks = evaluate(seed.kind, &traces);
    let passed = !checks.is_empty() && checks.iter().all(|c| c.passed);
    base(None, checks, passed)
}

/// Run the whole seeded-bug corpus.
pub fn run_corpus(bin: &Path, dir: &Path, timeout: Duration) -> io::Result<CorpusReport> {
    let seeds = builtin_seeds()
        .iter()
        .map(|s| run_seed(bin, dir, s, timeout))
        .collect();
    Ok(CorpusReport { seeds })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(label: &str, value: u32) -> UserTrace {
        UserTrace {
            machine: NODE_GATEWAY,
            label: label.into(),
            value,
        }
    }

    /// The real buggy-firmware trace (corrupt image booted, no rollback).
    fn buggy_trace() -> Vec<UserTrace> {
        vec![
            t("ota_state", 0),
            t("ota_state", 1),
            t("ota_state", 2),
            t("ota_crc_ok", 1),
            t("ota_state", 3),
            t("ota_slot", 1),
            t("ota_state", 4),
            t("ota_state", 5),
            t("ota_boot_result", 1),
        ]
    }

    /// The real fixed-firmware trace (corrupt image rolled back to slot A).
    fn fixed_trace() -> Vec<UserTrace> {
        vec![
            t("ota_state", 0),
            t("ota_state", 1),
            t("ota_state", 2),
            t("ota_crc_ok", 0),
            t("ota_state", 6),
            t("ota_rollback", 1),
            t("ota_active_slot", 0),
            t("ota_boot_result", 0),
        ]
    }

    #[test]
    fn parses_user_trace_line() {
        let line = r#"[machine.1]            0 user-u32 "ota_state" = 3"#;
        let e = parse_user_trace_line(line).unwrap();
        assert_eq!(e.machine, NODE_GATEWAY);
        assert_eq!(e.label, "ota_state");
        assert_eq!(e.value, 3);
    }

    #[test]
    fn ota_rollback_seed_passes_on_real_trace_pair() {
        let traces = SeedTraces {
            failing: buggy_trace(),
            fixed: fixed_trace(),
        };
        let checks = evaluate(SeedKind::OtaRollback, &traces);
        assert_eq!(checks.len(), 3);
        for c in &checks {
            assert!(c.passed, "{}: {}", c.name, c.detail);
        }
    }

    /// A SERVICE-mode motor-command trace event on the powertrain, packed as the
    /// firmware emits it: `(torque << 8) | motor_enable`.
    fn mc(torque: i8, motor_enable: bool) -> UserTrace {
        UserTrace {
            machine: NODE_POWERTRAIN,
            label: "diag_motor_command".into(),
            value: ((torque as u8 as u32) << 8) | (motor_enable as u32),
        }
    }

    // The real buggy trace: SERVICE session still commands 80% torque, motor on.
    fn service_bug_trace() -> Vec<UserTrace> {
        vec![mc(80, true), mc(80, true), mc(80, true)]
    }

    // The real fixed trace: SERVICE clamps torque to 0 with the motor disabled.
    fn service_fixed_trace() -> Vec<UserTrace> {
        vec![mc(0, false), mc(0, false), mc(0, false)]
    }

    #[test]
    fn decodes_packed_motor_command() {
        // 20481 = (80 << 8) | 1 — the real firmware value for torque 80, motor on.
        let cmds = service_motor_commands(&[UserTrace {
            machine: NODE_POWERTRAIN,
            label: "diag_motor_command".into(),
            value: 20481,
        }]);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].torque, 80);
        assert!(cmds[0].motor_enable);
    }

    #[test]
    fn service_torque_seed_passes_on_real_trace_pair() {
        let traces = SeedTraces {
            failing: service_bug_trace(),
            fixed: service_fixed_trace(),
        };
        let checks = evaluate(SeedKind::ServiceTorqueClamp, &traces);
        assert_eq!(checks.len(), 3);
        for c in &checks {
            assert!(c.passed, "{}: {}", c.name, c.detail);
        }
    }

    #[test]
    fn service_torque_fails_when_buggy_firmware_actually_clamps() {
        // If the "buggy" firmware clamped, the bug did not reproduce.
        let traces = SeedTraces {
            failing: service_fixed_trace(),
            fixed: service_fixed_trace(),
        };
        let checks = evaluate(SeedKind::ServiceTorqueClamp, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-reproduced")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn service_torque_fails_when_fixed_firmware_still_drives() {
        // If the "fixed" firmware still commands torque, it is not fixed.
        let traces = SeedTraces {
            failing: service_bug_trace(),
            fixed: service_bug_trace(),
        };
        let checks = evaluate(SeedKind::ServiceTorqueClamp, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-fixed")
                .unwrap()
                .passed
        );
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "traces-diverge")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn fails_when_buggy_firmware_actually_rolls_back() {
        // If the "buggy" firmware rolled back, the bug did not reproduce.
        let traces = SeedTraces {
            failing: fixed_trace(),
            fixed: fixed_trace(),
        };
        let checks = evaluate(SeedKind::OtaRollback, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-reproduced")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn fails_when_fixed_firmware_still_boots_bad_image() {
        // If the "fixed" firmware still boots the corrupt image, it is not fixed.
        let traces = SeedTraces {
            failing: buggy_trace(),
            fixed: buggy_trace(),
        };
        let checks = evaluate(SeedKind::OtaRollback, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-fixed")
                .unwrap()
                .passed
        );
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "traces-diverge")
                .unwrap()
                .passed
        );
    }

    /// A gateway_diag_response for a READ_DTCS request with the given req + count.
    fn dtc_resp(req: u8, count: u8) -> UserTrace {
        UserTrace {
            machine: NODE_GATEWAY,
            label: "gateway_diag_response".into(),
            value: ((req as u32) << 24) | ((DIAG_READ_DTCS as u32) << 16) | (count as u32),
        }
    }

    // Real buggy trace: 2 DTCs before the clear, 0 after (everything cleared).
    fn clear_bug_trace() -> Vec<UserTrace> {
        vec![
            dtc_resp(REQ_READ_DTCS_PRE, 2),
            dtc_resp(REQ_READ_DTCS_POST, 0),
        ]
    }

    // Real fixed trace: 2 DTCs before the clear, 1 after (powertrain survives).
    fn clear_fixed_trace() -> Vec<UserTrace> {
        vec![
            dtc_resp(REQ_READ_DTCS_PRE, 2),
            dtc_resp(REQ_READ_DTCS_POST, 1),
        ]
    }

    #[test]
    fn clear_all_dtcs_decodes_response_count() {
        assert_eq!(dtc_count(&[dtc_resp(5, 1)], REQ_READ_DTCS_POST), Some(1));
        assert_eq!(dtc_count(&[dtc_resp(3, 2)], REQ_READ_DTCS_PRE), Some(2));
    }

    #[test]
    fn clear_all_dtcs_seed_passes_on_real_trace_pair() {
        let traces = SeedTraces {
            failing: clear_bug_trace(),
            fixed: clear_fixed_trace(),
        };
        let checks = evaluate(SeedKind::ClearAllDtcs, &traces);
        assert_eq!(checks.len(), 3);
        for c in &checks {
            assert!(c.passed, "{}: {}", c.name, c.detail);
        }
    }

    #[test]
    fn clear_all_dtcs_fails_when_buggy_firmware_scopes_clear() {
        // If the "buggy" firmware only cleared BMS (post-clear 1), the bug did
        // not reproduce.
        let traces = SeedTraces {
            failing: clear_fixed_trace(),
            fixed: clear_fixed_trace(),
        };
        let checks = evaluate(SeedKind::ClearAllDtcs, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-reproduced")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn clear_all_dtcs_fails_when_fixed_firmware_still_clears_all() {
        // If the "fixed" firmware still cleared everything (post-clear 0), it is
        // not fixed and the traces do not diverge.
        let traces = SeedTraces {
            failing: clear_bug_trace(),
            fixed: clear_bug_trace(),
        };
        let checks = evaluate(SeedKind::ClearAllDtcs, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-fixed")
                .unwrap()
                .passed
        );
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "traces-diverge")
                .unwrap()
                .passed
        );
    }

    /// A gateway_diag_response for a START_SESSION request (req 1) with the given
    /// status + mode (value0).
    fn start_session_resp(status: u8, mode: u8) -> UserTrace {
        UserTrace {
            machine: NODE_GATEWAY,
            label: "gateway_diag_response".into(),
            value: (1u32 << 24)
                | ((DIAG_START_SESSION as u32) << 16)
                | ((status as u32) << 8)
                | (mode as u32),
        }
    }

    // Real buggy trace: START_SESSION mid-drive accepted (OK, mode SERVICE).
    fn start_drive_bug_trace() -> Vec<UserTrace> {
        vec![start_session_resp(DIAG_OK, VEHICLE_SERVICE)]
    }

    // Real fixed trace: START_SESSION mid-drive rejected (REJECTED, mode DRIVE).
    fn start_drive_fixed_trace() -> Vec<UserTrace> {
        vec![start_session_resp(DIAG_REJECTED, VEHICLE_DRIVE)]
    }

    #[test]
    fn start_session_decodes_status_and_mode() {
        let (status, mode) =
            diag_response_status_value0(&[start_session_resp(DIAG_OK, VEHICLE_SERVICE)], 1, 1)
                .unwrap();
        assert_eq!(status, DIAG_OK);
        assert_eq!(mode, VEHICLE_SERVICE);
    }

    #[test]
    fn start_session_in_drive_seed_passes_on_real_trace_pair() {
        let traces = SeedTraces {
            failing: start_drive_bug_trace(),
            fixed: start_drive_fixed_trace(),
        };
        let checks = evaluate(SeedKind::StartSessionInDrive, &traces);
        assert_eq!(checks.len(), 3);
        for c in &checks {
            assert!(c.passed, "{}: {}", c.name, c.detail);
        }
    }

    #[test]
    fn start_session_in_drive_fails_when_buggy_firmware_rejects() {
        // If the "buggy" firmware rejected the mid-drive session, the bug did
        // not reproduce.
        let traces = SeedTraces {
            failing: start_drive_fixed_trace(),
            fixed: start_drive_fixed_trace(),
        };
        let checks = evaluate(SeedKind::StartSessionInDrive, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-reproduced")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn start_session_in_drive_fails_when_fixed_firmware_still_accepts() {
        // If the "fixed" firmware still accepts the mid-drive session, it is not
        // fixed and the traces do not diverge.
        let traces = SeedTraces {
            failing: start_drive_bug_trace(),
            fixed: start_drive_bug_trace(),
        };
        let checks = evaluate(SeedKind::StartSessionInDrive, &traces);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "bug-fixed")
                .unwrap()
                .passed
        );
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "traces-diverge")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn report_totals_and_json() {
        let report = CorpusReport {
            seeds: vec![SeedResult {
                name: "ota_rollback".into(),
                description: "d".into(),
                symptom: "s".into(),
                primitive: "p".into(),
                passed: true,
                run_error: None,
                checks: vec![CorpusCheck {
                    name: "bug-reproduced".into(),
                    passed: true,
                    detail: "ok".into(),
                }],
            }],
        };
        assert_eq!(report.totals(), (1, 0));
        assert!(report.passed());
        let json = report.to_json().to_pretty();
        assert!(json.contains("debug_gym_corpus"));
        assert!(json.contains("ota_rollback"));
    }
}
