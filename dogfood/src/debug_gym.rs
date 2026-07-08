//! debug_gym lane — deterministic-debugging invariants.
//!
//! Purpose (docs/costar_microcar_dogfood_plan.md, "Dogfood Lanes > 5. debug_gym"):
//! turn microcar into an AI-debugging benchmark by asserting the determinism
//! properties a debugger relies on. This first cut wires the harness assertions
//! that the M7 costar primitives (`World::step`, `run_until`) make checkable:
//!
//! * **run-to-completion trace equals stepped trace** — running a scenario
//!   normally vs. with `--step` (which drives `World::step()` one event at a
//!   time) must produce byte-identical traces. This is guaranteed by
//!   construction in costar (run/run_until delegate to step) and verified here
//!   end-to-end through the product binary.
//! * **`run_until` never overshoots** — no trace event has a virtual time past
//!   the scenario's `duration_ms` deadline.
//! * **clock never moves backward** — the segment-aware
//!   [`VirtualTimeMonotonic`](crate::invariants::VirtualTimeMonotonic) check.
//!
//! The seeded-bug corpus (golden failing/fixed traces) and keyframe-restore
//! replay are later additions on top of these primitives.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::invariants::{CheckStatus, Invariant, VirtualTimeMonotonic};
use crate::json::Json;
use crate::runner::{run_scenario, run_scenario_args, RunStatus};
use crate::trace_hash::normalized_hash;

/// Default scenarios exercised by `harness debug-gym` when none are given:
/// short, deterministic vehicle scenarios (no soak/long runs).
pub const DEFAULT_SCENARIOS: &[&str] = &[
    "scenarios/boot_and_heartbeat.toml",
    "scenarios/normal_drive_cycle.toml",
    "scenarios/bms_overtemp_limp_mode.toml",
    "scenarios/brake_overrides_throttle.toml",
];

/// debug_gym result for one scenario.
#[derive(Debug, Clone)]
pub struct DebugGymScenarioResult {
    pub name: String,
    pub stepped_equals_continuous: bool,
    pub continuous_hash: String,
    pub stepped_hash: String,
    pub deadline: Option<u64>,
    pub max_trace_time: u64,
    pub no_overshoot: bool,
    pub clock_monotonic: bool,
    pub passed: bool,
    pub detail: String,
}

/// The whole debug_gym lane result.
#[derive(Debug, Clone)]
pub struct DebugGymReport {
    pub scenarios: Vec<DebugGymScenarioResult>,
}

impl DebugGymReport {
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
                Json::Obj(vec![
                    ("name".into(), Json::str(&s.name)),
                    (
                        "stepped_equals_continuous".into(),
                        Json::Bool(s.stepped_equals_continuous),
                    ),
                    ("continuous_hash".into(), Json::str(&s.continuous_hash)),
                    ("stepped_hash".into(), Json::str(&s.stepped_hash)),
                    (
                        "deadline".into(),
                        match s.deadline {
                            Some(d) => Json::UInt(d as u128),
                            None => Json::str("none"),
                        },
                    ),
                    ("max_trace_time".into(), Json::UInt(s.max_trace_time as u128)),
                    ("no_overshoot".into(), Json::Bool(s.no_overshoot)),
                    ("clock_monotonic".into(), Json::Bool(s.clock_monotonic)),
                    ("passed".into(), Json::Bool(s.passed)),
                    ("detail".into(), Json::str(&s.detail)),
                ])
            })
            .collect();
        let (passed, failed) = self.totals();
        Json::Obj(vec![
            ("lane".into(), Json::str("debug_gym")),
            ("passed".into(), Json::Bool(self.passed())),
            ("scenarios_passed".into(), Json::UInt(passed as u128)),
            ("scenarios_failed".into(), Json::UInt(failed as u128)),
            ("scenarios".into(), Json::Arr(scenarios)),
        ])
    }
}

/// Parse `duration_ms = N` from a scenario TOML (first match). Std-only, no TOML
/// parse needed.
pub fn parse_duration_ms(path: &Path) -> Option<u64> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("duration_ms") {
            if let Some((_, val)) = rest.split_once('=') {
                if let Ok(n) = val.trim().parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Max virtual time across `[machine.N] <time> ...` trace lines (0 if none).
pub fn max_trace_time(trace: &[String]) -> u64 {
    let mut max = 0u64;
    for line in trace {
        let t = line.trim_start();
        if !t.starts_with("[machine.") {
            continue;
        }
        let mut it = t.split_whitespace();
        let _prefix = it.next();
        if let Some(tok) = it.next() {
            if let Ok(v) = tok.parse::<u64>() {
                if v > max {
                    max = v;
                }
            }
        }
    }
    max
}

/// Run the debug_gym checks for `scenarios` through `bin`.
pub fn run_debug_gym(bin: &Path, scenarios: &[PathBuf], timeout: Duration) -> DebugGymReport {
    let results = scenarios
        .iter()
        .map(|s| run_one(bin, s, timeout))
        .collect();
    DebugGymReport { scenarios: results }
}

fn run_one(bin: &Path, scenario: &Path, timeout: Duration) -> DebugGymScenarioResult {
    let continuous = run_scenario(bin, scenario, timeout);
    let stepped = run_scenario_args(bin, scenario, timeout, &["--step"]);
    let name = continuous.scenario.clone();

    let continuous_hash = normalized_hash(&continuous.trace);
    let stepped_hash = normalized_hash(&stepped.trace);

    let clean = continuous.status == RunStatus::Pass && stepped.status == RunStatus::Pass;
    let stepped_equals_continuous = clean && continuous_hash == stepped_hash;

    let deadline = parse_duration_ms(scenario).map(|ms| ms * 1000);
    let max_t = max_trace_time(&continuous.trace);
    let no_overshoot = deadline.map(|d| max_t <= d).unwrap_or(true);

    let mono = VirtualTimeMonotonic.check(&continuous);
    let clock_monotonic = mono.status != CheckStatus::Fail;

    let passed = clean && stepped_equals_continuous && no_overshoot && clock_monotonic;

    let detail = if passed {
        format!(
            "stepped==continuous (hash {}), max_time {}{}, clock monotonic",
            &continuous_hash[..continuous_hash.len().min(12)],
            max_t,
            deadline.map(|d| format!(" <= deadline {d}")).unwrap_or_default(),
        )
    } else if !clean {
        format!(
            "run not clean: continuous={} stepped={}",
            continuous.status.as_str(),
            stepped.status.as_str()
        )
    } else if !stepped_equals_continuous {
        format!("stepped != continuous ({stepped_hash} vs {continuous_hash})")
    } else if !no_overshoot {
        format!("run_until overshoot: max_time {max_t} > deadline {deadline:?}")
    } else {
        format!("clock moved backward: {}", mono.message)
    };

    DebugGymScenarioResult {
        name,
        stepped_equals_continuous,
        continuous_hash,
        stepped_hash,
        deadline,
        max_trace_time: max_t,
        no_overshoot,
        clock_monotonic,
        passed,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_ms_reads_value() {
        let dir = std::env::temp_dir().join(format!("dg_dur_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.toml");
        std::fs::write(&f, "name = \"x\"\nduration_ms = 2000\n[[machine]]\nid=1\nname=\"a\"\n")
            .unwrap();
        assert_eq!(parse_duration_ms(&f), Some(2000));
        let g = dir.join("n.toml");
        std::fs::write(&g, "name = \"x\"\n").unwrap();
        assert_eq!(parse_duration_ms(&g), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn max_trace_time_finds_max() {
        let trace = vec![
            "[machine.1]        10 can-rx id=0x0102".to_string(),
            "[machine.2]   2990500 can-rx id=0x0102".to_string(),
            "[machine.1]       500 task-yield".to_string(),
            "PASS".to_string(),
        ];
        assert_eq!(max_trace_time(&trace), 2_990_500);
        assert_eq!(max_trace_time(&[]), 0);
    }
}
