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

/// Which built-in bug a seed represents (selects the assertion logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedKind {
    /// OTA rollback bug: a broken CRC check accepts a corrupt image, so the
    /// update boots the bad slot instead of rolling back.
    OtaRollback,
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
    vec![Seed {
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
    }]
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
    }
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
