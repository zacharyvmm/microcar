//! Subprocess-driven scenario runner.
//!
//! # Why subprocess isolation?
//!
//! The simulator's `World` type is `!Send`, so it cannot be moved onto a
//! timeout/watchdog thread; we can't wrap an in-process `run_until` in a
//! `thread::spawn(...).join_timeout(...)`. Spawning the `microcar` binary as a
//! child process sidesteps that entirely and, as a bonus, gives us:
//!
//! * **Real wall-clock timeout enforcement** — a runaway sim is `kill()`ed.
//! * **Panic isolation** — a child panic can't unwind our harness; we detect it
//!   from the exit status + stderr.
//!
//! # Timeout implementation (std-only, no `wait-timeout` crate)
//!
//! We pipe the child's stdout/stderr and drain each on its own reader thread so
//! the OS pipe buffers can't fill and deadlock the child. The main thread polls
//! [`Child::try_wait`] against a deadline; if the deadline passes while the
//! child is still alive we `kill()` it, mark the run `Timeout`, then join the
//! reader threads (which finish once the pipes close).

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How many trailing stdout/stderr lines to retain for diagnostics.
const TAIL_LINES: usize = 20;
/// Poll granularity while waiting on the child.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Terminal outcome of a scenario run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Child exited 0.
    Pass,
    /// Child exited non-zero without a detected panic.
    Fail,
    /// Child exceeded the wall-clock timeout and was killed.
    Timeout,
    /// Child exited non-zero AND stderr mentions a panic.
    Panic,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Pass => "PASS",
            RunStatus::Fail => "FAIL",
            RunStatus::Timeout => "TIMEOUT",
            RunStatus::Panic => "PANIC",
        }
    }

    /// Did the process terminate cleanly (exit 0, no timeout, no panic)?
    pub fn is_clean(self) -> bool {
        matches!(self, RunStatus::Pass)
    }
}

/// Result of running one scenario through the microcar binary.
#[derive(Debug, Clone)]
pub struct ScenarioRun {
    /// Scenario name, derived from the file stem.
    pub scenario: String,
    /// Terminal outcome.
    pub status: RunStatus,
    /// Child exit code, if it exited normally (None if killed/signalled).
    pub exit_code: Option<i32>,
    /// Trace event lines from stdout (header, blank lines, and the terminal
    /// `PASS`/`FAIL` marker are filtered out — this is the trace proper).
    pub trace: Vec<String>,
    /// Wall-clock duration of the run.
    pub wall: Duration,
    /// Last [`TAIL_LINES`] raw stdout lines (for human-facing diagnostics).
    pub stdout_tail: Vec<String>,
    /// Last [`TAIL_LINES`] stderr lines (for human-facing diagnostics).
    pub stderr_tail: Vec<String>,
}

impl ScenarioRun {
    pub fn wall_ms(&self) -> u128 {
        self.wall.as_millis()
    }
}

/// Run `scenario` through `microcar_bin`, enforcing a wall-clock `timeout`.
///
/// The child inherits the harness's current working directory, so relative
/// paths inside the scenario TOML (firmware, expected traces) resolve exactly as
/// they would when running microcar directly. Run the harness from the microcar
/// repo root (or set `MICROCAR_BIN` and pass an absolute scenario path).
///
/// Never panics: a spawn failure is reported as [`RunStatus::Fail`] with the OS
/// error in `stderr_tail`.
pub fn run_scenario(microcar_bin: &Path, scenario: &Path, timeout: Duration) -> ScenarioRun {
    run_scenario_args(microcar_bin, scenario, timeout, &[])
}

/// Like [`run_scenario`] but passes `extra_args` to the microcar binary after
/// the scenario path (e.g. `["--step"]` or `["--trace-v2", path]`).
pub fn run_scenario_args(
    microcar_bin: &Path,
    scenario: &Path,
    timeout: Duration,
    extra_args: &[&str],
) -> ScenarioRun {
    let name = scenario
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| scenario.to_string_lossy().into_owned());

    let start = Instant::now();

    let mut child = match Command::new(microcar_bin)
        .arg(scenario)
        .args(extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return ScenarioRun {
                scenario: name,
                status: RunStatus::Fail,
                exit_code: None,
                trace: Vec::new(),
                wall: start.elapsed(),
                stdout_tail: Vec::new(),
                stderr_tail: vec![format!("failed to spawn {}: {e}", microcar_bin.display())],
            };
        }
    };

    // Drain both pipes concurrently so the child never blocks on a full buffer.
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let out_handle = thread::spawn(move || read_all_lines(stdout));
    let err_handle = thread::spawn(move || read_all_lines(stderr));

    // Poll for exit against the deadline.
    let deadline = start + timeout;
    let mut timed_out = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(_) => break None,
        }
    };

    let stdout_lines = out_handle.join().unwrap_or_default();
    let stderr_lines = err_handle.join().unwrap_or_default();
    let wall = start.elapsed();

    let success = exit_status.map(|s| s.success()).unwrap_or(false);
    let exit_code = exit_status.and_then(|s| s.code());
    let panicked = stderr_lines.iter().any(|l| l.contains("panicked"));

    let status = if timed_out {
        RunStatus::Timeout
    } else if success {
        RunStatus::Pass
    } else if panicked {
        RunStatus::Panic
    } else {
        RunStatus::Fail
    };

    let trace: Vec<String> = stdout_lines
        .iter()
        .filter(|l| is_trace_event(l))
        .cloned()
        .collect();

    ScenarioRun {
        scenario: name,
        status,
        exit_code,
        trace,
        wall,
        stdout_tail: tail(&stdout_lines, TAIL_LINES),
        stderr_tail: tail(&stderr_lines, TAIL_LINES),
    }
}

/// True for real trace event lines; false for the `=== name ===` header, blank
/// lines, and the terminal `PASS`/`FAIL:` marker.
fn is_trace_event(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    if t.starts_with("=== ") && t.ends_with(" ===") {
        return false;
    }
    if t == "PASS" || t.starts_with("FAIL") {
        return false;
    }
    true
}

fn read_all_lines<R: Read>(r: R) -> Vec<String> {
    let mut out = Vec::new();
    for line in BufReader::new(r).lines() {
        match line {
            Ok(l) => out.push(l),
            Err(_) => break,
        }
    }
    out
}

fn tail(v: &[String], n: usize) -> Vec<String> {
    let start = v.len().saturating_sub(n);
    v[start..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_header_blank_and_pass() {
        assert!(!is_trace_event("=== normal_drive_cycle ==="));
        assert!(!is_trace_event(""));
        assert!(!is_trace_event("   "));
        assert!(!is_trace_event("PASS"));
        assert!(!is_trace_event("FAIL: trace mismatch"));
        assert!(is_trace_event("[machine.1] 10 can-rx id=0x0102 len=2"));
    }

    #[test]
    fn tail_keeps_last_n() {
        let v: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        let t = tail(&v, 3);
        assert_eq!(
            t,
            vec!["47".to_string(), "48".to_string(), "49".to_string()]
        );
    }

    #[test]
    fn tail_handles_short_input() {
        let v = vec!["a".to_string()];
        assert_eq!(tail(&v, 10), v);
    }

    #[test]
    fn run_status_clean_only_for_pass() {
        assert!(RunStatus::Pass.is_clean());
        assert!(!RunStatus::Fail.is_clean());
        assert!(!RunStatus::Timeout.is_clean());
        assert!(!RunStatus::Panic.is_clean());
    }

    #[test]
    fn missing_binary_is_reported_as_fail_not_panic() {
        let run = run_scenario(
            Path::new("/nonexistent/definitely/not/microcar"),
            Path::new("scenarios/whatever.toml"),
            Duration::from_secs(1),
        );
        assert_eq!(run.status, RunStatus::Fail);
        assert!(run.trace.is_empty());
        assert!(run
            .stderr_tail
            .iter()
            .any(|l| l.contains("failed to spawn")));
    }
}
