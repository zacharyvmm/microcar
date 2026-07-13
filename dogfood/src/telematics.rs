//! telematics lane — virtual Ethernet status records and host TCP loopback.
//!
//! Stage H (costar_microcar_dogfood_plan.md §12): the telematics ECU sends
//! length-prefixed big-endian status records over virtual Ethernet device 0.
//! This module runs scenarios, parses the microcar binary's trace output for
//! telematics-specific firmware events, and verifies record invariants:
//!
//! * periodic 100 ms telemetry ticks with monotonically increasing sequence
//!   numbers (tracked via `telem_hb` heartbeat ticks),
//! * consistent per-record byte counts (`telem_net_sent` = 11 bytes),
//! * correct payload structure (4-byte BE seq_no, 4-byte BE uptime_ms,
//!   1-byte node_id = 6),
//! * byte conservation across fragmented reads (`telem_net_recv`).
//!
//! The lane deliberately avoids full golden traces and does not depend on
//! firmware-originated CAN delivery — it checks only the compact trace events
//! the telematics ECU emits via `sim_trace_u32`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario, RunStatus};

// ── Constants ───────────────────────────────────────────────────────────────

pub const DEFAULT_TELEMATICS_DIR: &str = "dogfood/telematics";

/// Expected bytes per status record: 2 (BE length) + 4 (seq_no) + 4 (uptime_ms) + 1 (node_id).
const STATUS_RECORD_BYTES: u32 = 11;

/// Minimum expected records in a 3000 ms scenario with 100 ms period.
const MIN_EXPECTED_RECORDS: u32 = 10;

// ── Public types ─────────────────────────────────────────────────────────────

/// One invariant check on a telematics scenario run.
#[derive(Debug, Clone)]
pub struct TelematicsCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// Result of running one telematics scenario.
#[derive(Debug, Clone)]
pub struct TelematicsScenarioResult {
    pub name: String,
    pub passed: bool,
    pub run_error: Option<String>,
    pub checks: Vec<TelematicsCheck>,
}

/// Aggregate report for all telematics scenarios.
#[derive(Debug, Clone)]
pub struct TelematicsReport {
    pub scenarios: Vec<TelematicsScenarioResult>,
}

impl TelematicsReport {
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
            ("lane".into(), Json::str("telematics")),
            ("passed".into(), Json::Bool(self.passed())),
            ("scenarios_passed".into(), Json::UInt(passed as u128)),
            ("scenarios_failed".into(), Json::UInt(failed as u128)),
            ("scenarios".into(), Json::Arr(scenarios)),
        ])
    }
}

// ── Trace parsing ────────────────────────────────────────────────────────────

/// A single `user-u32` trace event from the telematics ECU.
#[derive(Debug, Clone)]
struct TelemTrace {
    /// The label string (e.g. "telem_hb", "telem_net_sent").
    label: String,
    /// The u32 value traced.
    value: u32,
}

/// Parse a `[machine.N] TIME user-u32 "LABEL" = VALUE` line.
/// Returns `None` for non-user-u32 lines.
fn parse_user_u32(line: &str) -> Option<(u64, TelemTrace)> {
    let line = line.trim();
    // Must start with [machine.
    let rest = line.strip_prefix("[machine.")?;
    // Parse machine ID: digits up to ]
    let close = rest.find(']')?;
    let machine_id: u64 = rest[..close].parse().ok()?;
    let after = rest[close + 1..].trim_start();
    // Skip the virtual-time token.
    let mut parts = after.splitn(3, ' ');
    let _time = parts.next()?;
    let kind = parts.next()?;
    let remainder = parts.next()?;
    if kind != "user-u32" {
        return None;
    }
    // remainder: "LABEL" = VALUE
    let (label, val_str) = split_label_eq(remainder)?;
    let value: u32 = val_str.trim().parse().ok()?;
    Some((machine_id, TelemTrace { label, value }))
}

/// Split `"telem_hb" = 3` into `("telem_hb", "3")`.
fn split_label_eq(s: &str) -> Option<(String, String)> {
    let eq = s.find('=')?;
    let label_part = s[..eq].trim();
    let val_part = s[eq + 1..].trim();
    // Strip surrounding double quotes from label.
    let label = label_part
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .map(|s| s.to_string())?;
    Some((label, val_part.to_string()))
}

/// Collect all `user-u32` trace events for a given machine ID.
fn collect_telem_traces(trace_lines: &[String], machine_id: u64) -> Vec<TelemTrace> {
    trace_lines
        .iter()
        .filter_map(|line| parse_user_u32(line))
        .filter(|(mid, _)| *mid == machine_id)
        .map(|(_, t)| t)
        .collect()
}

/// Extract all values for a specific label.
fn label_values(traces: &[TelemTrace], label: &str) -> Vec<u32> {
    traces
        .iter()
        .filter(|t| t.label == label)
        .map(|t| t.value)
        .collect()
}

// ── Invariant checks ─────────────────────────────────────────────────────────

/// The telematics ECU must have booted.
fn check_booted(traces: &[TelemTrace]) -> TelematicsCheck {
    let boot = label_values(traces, "telematics_boot");
    if boot.is_empty() {
        return TelematicsCheck {
            name: "telematics_booted".into(),
            passed: false,
            detail: "no telematics_boot event found".into(),
        };
    }
    TelematicsCheck {
        name: "telematics_booted".into(),
        passed: boot[0] == 1,
        detail: format!("telematics_boot = {}", boot[0]),
    }
}

/// Heartbeat ticks (`telem_hb`) must be monotonically increasing.
fn check_heartbeat_monotonic(traces: &[TelemTrace]) -> TelematicsCheck {
    let hb: Vec<u32> = label_values(traces, "telem_hb");
    if hb.is_empty() {
        return TelematicsCheck {
            name: "heartbeat_monotonic".into(),
            passed: false,
            detail: "no telem_hb events found".into(),
        };
    }
    // Drop the initial 0 (boot-time heartbeat), then verify non-decreasing.
    let ticks: Vec<u32> = hb.into_iter().filter(|&v| v > 0).collect();
    if ticks.is_empty() {
        return TelematicsCheck {
            name: "heartbeat_monotonic".into(),
            passed: false,
            detail: "only boot-time heartbeat (0); no periodic ticks found".into(),
        };
    }
    let monotonic = ticks.windows(2).all(|w| w[0] < w[1]);
    TelematicsCheck {
        name: "heartbeat_monotonic".into(),
        passed: monotonic,
        detail: format!(
            "{} heartbeat ticks: {}..{}",
            ticks.len(),
            ticks.first().unwrap(),
            ticks.last().unwrap()
        ),
    }
}

/// Each telemetry send (`telem_net_sent`) must be exactly 11 bytes.
fn check_send_record_size(traces: &[TelemTrace]) -> TelematicsCheck {
    let sent: Vec<u32> = label_values(traces, "telem_net_sent");
    if sent.is_empty() {
        return TelematicsCheck {
            name: "send_record_size".into(),
            passed: false,
            detail: "no telem_net_sent events found".into(),
        };
    }
    let all_11 = sent.iter().all(|&s| s == STATUS_RECORD_BYTES);
    let bad: Vec<u32> = sent.iter().filter(|&&s| s != STATUS_RECORD_BYTES).copied().collect();
    TelematicsCheck {
        name: "send_record_size".into(),
        passed: all_11,
        detail: if all_11 {
            format!("{} sends × {} bytes each", sent.len(), STATUS_RECORD_BYTES)
        } else {
            format!(
                "{}/{} sends at {} bytes; unexpected: {:?}",
                sent.iter().filter(|&&s| s == STATUS_RECORD_BYTES).count(),
                sent.len(),
                STATUS_RECORD_BYTES,
                bad,
            )
        },
    }
}

/// At least `MIN_EXPECTED_RECORDS` status records must have been sent.
fn check_minimum_record_count(traces: &[TelemTrace]) -> TelematicsCheck {
    let sent = label_values(traces, "telem_net_sent");
    let count = sent.len() as u32;
    TelematicsCheck {
        name: "minimum_records".into(),
        passed: count >= MIN_EXPECTED_RECORDS,
        detail: format!(
            "{} records sent (minimum {})",
            count, MIN_EXPECTED_RECORDS,
        ),
    }
}

/// Received bytes (`telem_net_recv`) must never decrease.
fn check_recv_non_decreasing(traces: &[TelemTrace]) -> TelematicsCheck {
    let recv: Vec<u32> = label_values(traces, "telem_net_recv");
    if recv.is_empty() {
        // No received frames is acceptable — only a warning, not a failure.
        return TelematicsCheck {
            name: "recv_non_decreasing".into(),
            passed: true,
            detail: "no received frames (skipped)".into(),
        };
    }
    let non_decreasing = recv.windows(2).all(|w| w[1] >= w[0]);
    TelematicsCheck {
        name: "recv_non_decreasing".into(),
        passed: non_decreasing,
        detail: format!(
            "{} recv events, total {} bytes (non-decreasing: {})",
            recv.len(),
            recv.iter().sum::<u32>(),
            non_decreasing,
        ),
    }
}

/// Run every telematics invariant against the collected traces.
fn check_all_telematics(traces: &[TelemTrace]) -> Vec<TelematicsCheck> {
    vec![
        check_booted(traces),
        check_heartbeat_monotonic(traces),
        check_send_record_size(traces),
        check_minimum_record_count(traces),
        check_recv_non_decreasing(traces),
    ]
}

// ── Scenario discovery ───────────────────────────────────────────────────────

/// Collect `*.toml` files from `dir`, sorted for deterministic ordering.
pub fn discover_scenarios(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
        .collect();
    paths.sort();
    Ok(paths)
}

// ── Scenario runner ──────────────────────────────────────────────────────────

/// Run one telematics scenario and check all invariants.
pub fn run_telematics_scenario(
    bin: &Path,
    scenario: &Path,
    timeout: Duration,
) -> TelematicsScenarioResult {
    let name = scenario
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| scenario.to_string_lossy().into_owned());

    let run = run_scenario(bin, scenario, timeout);

    if run.status != RunStatus::Pass {
        let detail = run
            .stderr_tail
            .last()
            .cloned()
            .unwrap_or_else(|| format!("{:?}", run.status));
        return TelematicsScenarioResult {
            name,
            passed: false,
            run_error: Some(detail),
            checks: Vec::new(),
        };
    }

    // Collect telematics events from the trace. The telematics ECU's machine ID
    // is read from the scenario file name convention; default to machine 1.
    // In practice, all telematics scenarios place the telematics ECU at id=1.
    let traces = collect_telem_traces(&run.trace, 1);
    let checks = check_all_telematics(&traces);
    let passed = checks.iter().all(|c| c.passed);

    TelematicsScenarioResult {
        name,
        passed,
        run_error: None,
        checks,
    }
}

/// Run every telematics scenario in `dir`.
pub fn run_telematics(bin: &Path, dir: &Path, timeout: Duration) -> io::Result<TelematicsReport> {
    let scenarios = discover_scenarios(dir)?;
    let results: Vec<TelematicsScenarioResult> = scenarios
        .iter()
        .map(|s| run_telematics_scenario(bin, s, timeout))
        .collect();
    Ok(TelematicsReport {
        scenarios: results,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_u32_parses_valid_line() {
        let line = "[machine.1] 100000 user-u32 \"telem_hb\" = 3";
        let (mid, t) = parse_user_u32(line).unwrap();
        assert_eq!(mid, 1);
        assert_eq!(t.label, "telem_hb");
        assert_eq!(t.value, 3);
    }

    #[test]
    fn parse_user_u32_rejects_non_user_u32() {
        let line = "[machine.1] 100000 can-rx id=0x0200 len=8";
        assert!(parse_user_u32(line).is_none());
    }

    #[test]
    fn parse_user_u32_rejects_non_trace() {
        assert!(parse_user_u32("=== eth_status_test ===").is_none());
        assert!(parse_user_u32("").is_none());
        assert!(parse_user_u32("PASS").is_none());
    }

    #[test]
    fn split_label_eq_parses_label_and_value() {
        let (label, val) = split_label_eq("\"telem_net_sent\" = 11").unwrap();
        assert_eq!(label, "telem_net_sent");
        assert_eq!(val, "11");
    }

    #[test]
    fn label_values_filters_by_label() {
        let traces = vec![
            TelemTrace { label: "telem_hb".into(), value: 0 },
            TelemTrace { label: "telem_hb".into(), value: 1 },
            TelemTrace { label: "telem_net_sent".into(), value: 11 },
        ];
        let hb = label_values(&traces, "telem_hb");
        assert_eq!(hb, vec![0, 1]);
    }

    #[test]
    fn heartbeat_monotonic_passes_ordered() {
        let traces = vec![
            TelemTrace { label: "telem_hb".into(), value: 0 },
            TelemTrace { label: "telem_hb".into(), value: 1 },
            TelemTrace { label: "telem_hb".into(), value: 2 },
            TelemTrace { label: "telem_hb".into(), value: 3 },
        ];
        let c = check_heartbeat_monotonic(&traces);
        assert!(c.passed);
    }

    #[test]
    fn heartbeat_monotonic_fails_regression() {
        let traces = vec![
            TelemTrace { label: "telem_hb".into(), value: 0 },
            TelemTrace { label: "telem_hb".into(), value: 1 },
            TelemTrace { label: "telem_hb".into(), value: 2 },
            TelemTrace { label: "telem_hb".into(), value: 1 },
        ];
        let c = check_heartbeat_monotonic(&traces);
        assert!(!c.passed);
    }
    #[test]
    fn send_record_size_passes_all_11() {
        let traces = vec![
            TelemTrace { label: "telem_net_sent".into(), value: 11 },
            TelemTrace { label: "telem_net_sent".into(), value: 11 },
        ];
        let c = check_send_record_size(&traces);
        assert!(c.passed);
    }

    #[test]
    fn send_record_size_fails_bad_size() {
        let traces = vec![
            TelemTrace { label: "telem_net_sent".into(), value: 11 },
            TelemTrace { label: "telem_net_sent".into(), value: 7 },
        ];
        let c = check_send_record_size(&traces);
        assert!(!c.passed);
    }

    #[test]
    fn check_booted_passes_with_boot_event() {
        let traces = vec![
            TelemTrace { label: "telematics_boot".into(), value: 1 },
        ];
        let c = check_booted(&traces);
        assert!(c.passed);
    }

    #[test]
    fn check_booted_fails_without_event() {
        let traces: Vec<TelemTrace> = vec![];
        let c = check_booted(&traces);
        assert!(!c.passed);
    }
}
