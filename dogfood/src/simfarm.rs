//! simfarm lane — concurrent-determinism, churn, and panic-isolation.
//!
//! Implements dogfood lane #1 from `docs/costar_microcar_dogfood_plan.md`
//! ("Dogfood Lanes > 1. simfarm"). Its job is to prove three properties that a
//! long-lived, multi-session costar server will eventually have to guarantee:
//!
//! * **Concurrent determinism** — the same scenario, run many times *at once*,
//!   produces byte-identical normalized traces (and equal to a solo baseline).
//!   This is the concurrent extension of the sequential [`crate::determinism`]
//!   check: it catches process-global clocks, shared task-ID counters, and any
//!   other cross-session state that would make traces diverge under load.
//! * **Churn** — repeatedly create/run/destroy the scenario and confirm
//!   determinism stays stable across launches, i.e. no state leaks from one
//!   run into the next.
//! * **Panic isolation** — one intentionally malformed scenario must fail
//!   *cleanly* (structured error exit, not a crash) while a healthy scenario
//!   running concurrently finishes unchanged.
//!
//! # Subprocess model (and what that means for these checks)
//!
//! The simulator's `World` is `!Send` and each `microcar` process owns exactly
//! one `World` for one scenario, then exits. So this lane's "concurrency" is
//! across *processes*: we spawn N child `microcar` binaries at once (one per
//! [`std::thread`], each driving its own child via [`run_scenario`]). True
//! in-process per-session isolation inside a single long-lived server is a
//! later costar milestone; this lane can't exercise that yet. What it *can*
//! prove today is still valuable: (1) trace determinism holds under genuine
//! concurrent load, and (2) a bad input can't take down concurrent good runs.
//!
//! ## Why there is no RSS-plateau assertion
//!
//! The plan's churn description mentions "assert RSS plateaus". That is only
//! meaningful for an in-process, long-lived server that keeps allocating across
//! sessions. In this subprocess model every churn iteration is a *fresh,
//! short-lived process* that the OS reaps on exit, so there is no growing
//! resident set to plateau — measuring it would be noise. Instead churn asserts
//! stable determinism + all-clean across iterations, which is the property that
//! actually catches cross-launch state leakage under this model. We do not try
//! to measure RSS.

use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::json::Json;
use crate::runner::{run_scenario, RunStatus, ScenarioRun};
use crate::trace_hash::normalized_hash;

/// One concurrent run's normalized trace hash plus its outcome.
pub struct RunHash {
    /// Normalized FNV-1a trace hash for this run.
    pub hash: String,
    /// Terminal outcome of this run.
    pub status: RunStatus,
    /// Wall-clock duration in milliseconds.
    pub wall_ms: u128,
}

impl RunHash {
    fn from_run(run: &ScenarioRun) -> RunHash {
        RunHash {
            hash: normalized_hash(&run.trace),
            status: run.status,
            wall_ms: run.wall_ms(),
        }
    }

    fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("hash".into(), Json::str(&self.hash)),
            ("status".into(), Json::str(self.status.as_str())),
            ("wall_ms".into(), Json::UInt(self.wall_ms)),
        ])
    }
}

/// Report for the concurrent-determinism check.
pub struct SimfarmReport {
    pub scenario: String,
    pub n: usize,
    /// Canonical hash from the solo baseline run.
    pub solo_hash: String,
    /// One entry per concurrent run.
    pub concurrent: Vec<RunHash>,
    /// True iff every concurrent hash equals `solo_hash`.
    pub all_match: bool,
    /// True iff the solo run was `Pass` and every concurrent run was `Pass`.
    pub all_clean: bool,
}

impl SimfarmReport {
    /// The lane passes iff every concurrent trace matched the solo baseline and
    /// every run (solo + concurrent) terminated cleanly.
    pub fn passed(&self) -> bool {
        self.all_match && self.all_clean
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("lane".into(), Json::str("simfarm")),
            ("scenario".into(), Json::str(&self.scenario)),
            ("n".into(), Json::UInt(self.n as u128)),
            ("solo_hash".into(), Json::str(&self.solo_hash)),
            (
                "concurrent".into(),
                Json::Arr(self.concurrent.iter().map(RunHash::to_json).collect()),
            ),
            ("all_match".into(), Json::Bool(self.all_match)),
            ("all_clean".into(), Json::Bool(self.all_clean)),
            ("passed".into(), Json::Bool(self.passed())),
        ])
    }
}

/// Report for the churn check.
pub struct ChurnReport {
    pub scenario: String,
    pub iterations: usize,
    /// Hash of the first iteration (the baseline the rest are compared to).
    pub first_hash: String,
    /// Number of distinct normalized hashes seen across all iterations.
    pub distinct_hashes: usize,
    /// Number of iterations that terminated cleanly (`Pass`).
    pub clean: usize,
    /// True iff every iteration was clean and every hash was identical.
    pub stable: bool,
}

impl ChurnReport {
    /// The lane passes iff churn was stable (all-clean, single distinct hash).
    pub fn passed(&self) -> bool {
        self.stable
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("lane".into(), Json::str("churn")),
            ("scenario".into(), Json::str(&self.scenario)),
            ("iterations".into(), Json::UInt(self.iterations as u128)),
            ("first_hash".into(), Json::str(&self.first_hash)),
            ("distinct_hashes".into(), Json::UInt(self.distinct_hashes as u128)),
            ("clean".into(), Json::UInt(self.clean as u128)),
            ("stable".into(), Json::Bool(self.stable)),
            ("passed".into(), Json::Bool(self.passed())),
        ])
    }
}

/// Report for the panic-isolation check.
pub struct PanicIsolationReport {
    pub healthy_scenario: String,
    pub bad_scenario: String,
    /// Outcome of the healthy run while the bad run was live.
    pub healthy_status: RunStatus,
    /// Healthy run's concurrent trace hash.
    pub healthy_hash: String,
    /// Healthy run's solo baseline hash (what `healthy_hash` must equal).
    pub healthy_solo_hash: String,
    /// Outcome of the intentionally malformed run.
    pub bad_status: RunStatus,
    /// Exit code of the bad run, if it exited normally.
    pub bad_exit_code: Option<i32>,
    /// True iff the healthy run was unaffected and the bad run failed cleanly
    /// (i.e. did NOT `Panic`).
    pub isolated: bool,
}

impl PanicIsolationReport {
    /// The lane passes iff the healthy run was isolated from the bad one.
    pub fn passed(&self) -> bool {
        self.isolated
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("lane".into(), Json::str("panic_isolation")),
            ("healthy_scenario".into(), Json::str(&self.healthy_scenario)),
            ("bad_scenario".into(), Json::str(&self.bad_scenario)),
            ("healthy_status".into(), Json::str(self.healthy_status.as_str())),
            ("healthy_hash".into(), Json::str(&self.healthy_hash)),
            ("healthy_solo_hash".into(), Json::str(&self.healthy_solo_hash)),
            ("bad_status".into(), Json::str(self.bad_status.as_str())),
            ("bad_exit_code".into(), exit_code_json(self.bad_exit_code)),
            ("isolated".into(), Json::Bool(self.isolated)),
            ("passed".into(), Json::Bool(self.passed())),
        ])
    }
}

/// Run `scenario` once (solo) to get the canonical hash, then run `n` copies of
/// the SAME scenario concurrently (each its own child process on its own
/// thread) and assert every concurrent normalized trace hash equals the solo
/// hash and every concurrent run terminated cleanly.
///
/// `n` is clamped to a minimum of 1 (matching [`crate::determinism`]).
pub fn run_simfarm(bin: &Path, scenario: &Path, n: usize, timeout: Duration) -> SimfarmReport {
    let n = n.max(1);

    // Solo baseline first, sequentially, so it can't be perturbed by the fleet.
    let solo = run_scenario(bin, scenario, timeout);
    let scenario_name = solo.scenario.clone();
    let solo_hash = normalized_hash(&solo.trace);

    // Now the concurrent fleet: one child per thread.
    let runs = run_concurrent(bin, scenario, n, timeout);
    let concurrent: Vec<RunHash> = runs.iter().map(RunHash::from_run).collect();

    let all_match = concurrent.iter().all(|r| r.hash == solo_hash);
    let all_clean = solo.status == RunStatus::Pass && concurrent.iter().all(|r| r.status == RunStatus::Pass);

    SimfarmReport {
        scenario: scenario_name,
        n,
        solo_hash,
        concurrent,
        all_match,
        all_clean,
    }
}

/// Churn: launch the scenario `iterations` times (fresh child each time, run
/// sequentially so create/run/destroy happens in order). Assert every run is
/// clean and every normalized hash equals the first — i.e. no state leaks
/// across launches and determinism is stable under repeated create/run/destroy.
///
/// `iterations` is clamped to a minimum of 1.
pub fn run_churn(bin: &Path, scenario: &Path, iterations: usize, timeout: Duration) -> ChurnReport {
    let iterations = iterations.max(1);
    let mut scenario_name = scenario.to_string_lossy().into_owned();
    let mut hashes: Vec<String> = Vec::with_capacity(iterations);
    let mut clean = 0usize;

    for _ in 0..iterations {
        let run = run_scenario(bin, scenario, timeout);
        scenario_name = run.scenario.clone();
        if run.status == RunStatus::Pass {
            clean += 1;
        }
        hashes.push(normalized_hash(&run.trace));
    }

    let first_hash = hashes.first().cloned().unwrap_or_default();
    let distinct_hashes = count_distinct(&hashes);
    let stable = clean == iterations && distinct_hashes == 1;

    ChurnReport {
        scenario: scenario_name,
        iterations,
        first_hash,
        distinct_hashes,
        clean,
        stable,
    }
}

/// Panic-isolation: run a `healthy` scenario and a `bad` (malformed) scenario
/// concurrently. Assert the healthy run finishes cleanly with a hash equal to
/// its own solo hash, AND the bad run fails cleanly — i.e. its status is NOT
/// [`RunStatus::Panic`]. A malformed scenario makes the microcar binary exit 2
/// with a structured error ([`RunStatus::Fail`], `exit_code == Some(2)`); a
/// crash would surface as [`RunStatus::Panic`].
///
/// `isolated` is true iff `healthy_status == Pass && healthy_hash ==
/// healthy_solo_hash && bad_status != Panic`.
pub fn run_panic_isolation(bin: &Path, healthy: &Path, bad: &Path, timeout: Duration) -> PanicIsolationReport {
    // Solo baseline for the healthy scenario, established before the bad run
    // ever exists.
    let healthy_solo = run_scenario(bin, healthy, timeout);
    let healthy_scenario = healthy_solo.scenario.clone();
    let healthy_solo_hash = normalized_hash(&healthy_solo.trace);

    // Run healthy + bad at the same time, each its own child on its own thread.
    let healthy_run;
    let bad_run;
    {
        let (h_bin, h_scn) = (bin.to_path_buf(), healthy.to_path_buf());
        let (b_bin, b_scn) = (bin.to_path_buf(), bad.to_path_buf());
        let h_handle = thread::spawn(move || run_scenario(&h_bin, &h_scn, timeout));
        let b_handle = thread::spawn(move || run_scenario(&b_bin, &b_scn, timeout));
        healthy_run = h_handle.join().expect("run_scenario never panics");
        bad_run = b_handle.join().expect("run_scenario never panics");
    }

    let bad_scenario = bad_run.scenario.clone();
    let healthy_status = healthy_run.status;
    let healthy_hash = normalized_hash(&healthy_run.trace);
    let bad_status = bad_run.status;
    let bad_exit_code = bad_run.exit_code;

    let isolated = healthy_status == RunStatus::Pass
        && healthy_hash == healthy_solo_hash
        && bad_status != RunStatus::Panic;

    PanicIsolationReport {
        healthy_scenario,
        bad_scenario,
        healthy_status,
        healthy_hash,
        healthy_solo_hash,
        bad_status,
        bad_exit_code,
        isolated,
    }
}

/// Spawn `n` copies of the scenario concurrently, one child process per thread,
/// and collect their runs. `run_scenario` drains child stdout/stderr on its own
/// reader threads and never panics, so joining is safe.
fn run_concurrent(bin: &Path, scenario: &Path, n: usize, timeout: Duration) -> Vec<ScenarioRun> {
    let mut handles = Vec::with_capacity(n);
    for _ in 0..n {
        let bin = bin.to_path_buf();
        let scenario = scenario.to_path_buf();
        handles.push(thread::spawn(move || run_scenario(&bin, &scenario, timeout)));
    }
    handles
        .into_iter()
        .map(|h| h.join().expect("run_scenario never panics"))
        .collect()
}

/// Count distinct strings, preserving nothing but the count (std-only, no set).
fn count_distinct(hashes: &[String]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for h in hashes {
        if !seen.contains(&h.as_str()) {
            seen.push(h.as_str());
        }
    }
    seen.len()
}

/// Render an optional exit code as JSON. Real exit codes (0..=255) render as a
/// number; a signalled/absent code renders as the string `"none"` (the `Json`
/// type has no null).
fn exit_code_json(code: Option<i32>) -> Json {
    match code {
        Some(c) if c >= 0 => Json::UInt(c as u128),
        Some(c) => Json::str(c.to_string()),
        None => Json::str("none"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- pure-logic test builders (no subprocess spawning) ----------------

    fn rh(hash: &str, status: RunStatus) -> RunHash {
        RunHash { hash: hash.to_string(), status, wall_ms: 5 }
    }

    /// Mirror `run_simfarm`'s invariant computation over hand-built runs.
    fn simfarm_report(solo_hash: &str, solo_clean: bool, runs: Vec<RunHash>) -> SimfarmReport {
        let all_match = runs.iter().all(|r| r.hash == solo_hash);
        let all_clean = solo_clean && runs.iter().all(|r| r.status == RunStatus::Pass);
        SimfarmReport {
            scenario: "demo".into(),
            n: runs.len(),
            solo_hash: solo_hash.to_string(),
            concurrent: runs,
            all_match,
            all_clean,
        }
    }

    fn churn_report(hashes: &[&str], statuses: &[RunStatus]) -> ChurnReport {
        let owned: Vec<String> = hashes.iter().map(|s| s.to_string()).collect();
        let iterations = owned.len();
        let clean = statuses.iter().filter(|s| **s == RunStatus::Pass).count();
        let distinct_hashes = count_distinct(&owned);
        let first_hash = owned.first().cloned().unwrap_or_default();
        let stable = clean == iterations && distinct_hashes == 1;
        ChurnReport {
            scenario: "demo".into(),
            iterations,
            first_hash,
            distinct_hashes,
            clean,
            stable,
        }
    }

    fn panic_report(
        healthy_status: RunStatus,
        healthy_hash: &str,
        solo_hash: &str,
        bad_status: RunStatus,
        bad_exit_code: Option<i32>,
    ) -> PanicIsolationReport {
        let isolated = healthy_status == RunStatus::Pass
            && healthy_hash == solo_hash
            && bad_status != RunStatus::Panic;
        PanicIsolationReport {
            healthy_scenario: "good".into(),
            bad_scenario: "bad".into(),
            healthy_status,
            healthy_hash: healthy_hash.to_string(),
            healthy_solo_hash: solo_hash.to_string(),
            bad_status,
            bad_exit_code,
            isolated,
        }
    }

    // ---- simfarm ----------------------------------------------------------

    #[test]
    fn simfarm_passes_when_all_match_and_clean() {
        let r = simfarm_report(
            "abc",
            true,
            vec![rh("abc", RunStatus::Pass), rh("abc", RunStatus::Pass), rh("abc", RunStatus::Pass)],
        );
        assert!(r.all_match);
        assert!(r.all_clean);
        assert!(r.passed());
    }

    #[test]
    fn simfarm_fails_on_hash_mismatch() {
        let r = simfarm_report(
            "abc",
            true,
            vec![rh("abc", RunStatus::Pass), rh("xyz", RunStatus::Pass)],
        );
        assert!(!r.all_match);
        assert!(r.all_clean); // all clean, but hashes diverged under load
        assert!(!r.passed());
    }

    #[test]
    fn simfarm_fails_on_unclean_status() {
        let r = simfarm_report(
            "abc",
            true,
            vec![rh("abc", RunStatus::Pass), rh("abc", RunStatus::Timeout)],
        );
        assert!(r.all_match);
        assert!(!r.all_clean);
        assert!(!r.passed());
    }

    #[test]
    fn simfarm_json_has_lane_and_passed() {
        let r = simfarm_report("abc", true, vec![rh("abc", RunStatus::Pass)]);
        let s = r.to_json().to_pretty();
        assert!(s.contains("\"lane\": \"simfarm\""));
        assert!(s.contains("\"solo_hash\": \"abc\""));
        assert!(s.contains("\"passed\": true"));
        assert!(s.contains("\"status\": \"PASS\""));
        // Structurally balanced => valid JSON.
        assert_eq!(s.matches('{').count(), s.matches('}').count());
        assert_eq!(s.matches('[').count(), s.matches(']').count());
    }

    // ---- churn ------------------------------------------------------------

    #[test]
    fn churn_stable_vs_unstable() {
        let stable = churn_report(&["h", "h", "h"], &[RunStatus::Pass; 3]);
        assert_eq!(stable.distinct_hashes, 1);
        assert_eq!(stable.clean, 3);
        assert!(stable.stable);
        assert!(stable.passed());

        // A second distinct hash means state leaked across launches.
        let drift = churn_report(&["h", "h", "g"], &[RunStatus::Pass; 3]);
        assert_eq!(drift.distinct_hashes, 2);
        assert!(!drift.passed());

        // An unclean run also defeats churn even if all hashes match.
        let unclean = churn_report(
            &["h", "h", "h"],
            &[RunStatus::Pass, RunStatus::Pass, RunStatus::Fail],
        );
        assert_eq!(unclean.distinct_hashes, 1);
        assert_eq!(unclean.clean, 2);
        assert!(!unclean.passed());
    }

    #[test]
    fn churn_json_has_lane_and_passed() {
        let r = churn_report(&["h", "h"], &[RunStatus::Pass; 2]);
        let s = r.to_json().to_pretty();
        assert!(s.contains("\"lane\": \"churn\""));
        assert!(s.contains("\"first_hash\": \"h\""));
        assert!(s.contains("\"distinct_hashes\": 1"));
        assert!(s.contains("\"passed\": true"));
        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }

    // ---- panic isolation --------------------------------------------------

    #[test]
    fn panic_isolation_logic() {
        // Healthy unchanged + bad fails cleanly (exit 2) => isolated.
        let ok = panic_report(RunStatus::Pass, "h", "h", RunStatus::Fail, Some(2));
        assert!(ok.isolated);
        assert!(ok.passed());

        // A real crash in the bad run defeats isolation.
        let crashed = panic_report(RunStatus::Pass, "h", "h", RunStatus::Panic, None);
        assert!(!crashed.isolated);
        assert!(!crashed.passed());

        // Healthy trace drifting from its solo baseline defeats isolation,
        // even if the bad run failed cleanly.
        let drift = panic_report(RunStatus::Pass, "h2", "h", RunStatus::Fail, Some(2));
        assert!(!drift.isolated);

        // Healthy run itself not clean defeats isolation.
        let unhealthy = panic_report(RunStatus::Timeout, "h", "h", RunStatus::Fail, Some(2));
        assert!(!unhealthy.isolated);
    }

    #[test]
    fn panic_json_includes_exit_code_and_lane() {
        let r = panic_report(RunStatus::Pass, "h", "h", RunStatus::Fail, Some(2));
        let s = r.to_json().to_pretty();
        assert!(s.contains("\"lane\": \"panic_isolation\""));
        assert!(s.contains("\"bad_status\": \"FAIL\""));
        assert!(s.contains("\"bad_exit_code\": 2"));
        assert!(s.contains("\"isolated\": true"));
        assert!(s.contains("\"passed\": true"));

        // A signalled bad run (no exit code) renders as "none".
        let signalled = panic_report(RunStatus::Pass, "h", "h", RunStatus::Fail, None);
        assert!(signalled.to_json().to_pretty().contains("\"bad_exit_code\": \"none\""));

        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }
}
