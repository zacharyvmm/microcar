//! Invariant framework.
//!
//! An [`Invariant`] inspects a [`ScenarioRun`] and returns an [`InvariantResult`]
//! (`Pass` / `Fail` / `Skipped`). The plan (docs/costar_microcar_dogfood_plan.md,
//! "Dogfood Harness Foundation") lists ~10 invariants; only a few are checkable
//! from today's stdout trace, so the rest are wired in as `Skipped` stubs with a
//! clear TODO describing the richer trace data they'll need. Later lanes just add
//! `impl Invariant` blocks — the trait shape and [`check_all`] driver don't move.

use crate::runner::{RunStatus, ScenarioRun};

/// Outcome of a single invariant check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    /// Not evaluated (not applicable, or needs data the trace doesn't carry yet).
    Skipped,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Skipped => "SKIP",
        }
    }
}

/// Result of evaluating one invariant against one run.
#[derive(Debug, Clone)]
pub struct InvariantResult {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

impl InvariantResult {
    pub fn pass(name: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Pass,
            message: message.into(),
        }
    }
    pub fn fail(name: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Fail,
            message: message.into(),
        }
    }
    pub fn skipped(name: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Skipped,
            message: message.into(),
        }
    }
    pub fn is_fail(&self) -> bool {
        self.status == CheckStatus::Fail
    }
}

/// Something that can be checked against a completed run.
pub trait Invariant {
    fn name(&self) -> &str;
    fn check(&self, run: &ScenarioRun) -> InvariantResult;
}

// ─── Implemented invariants (checkable from stdout trace today) ─────────────

/// Virtual (simulated) time must never move backward *within a trace segment*.
///
/// microcar's trace is emitted grouped by machine, and within one machine it is
/// a concatenation of monotonic *segments* from distinct trace sinks: the World
/// sink (CAN/bus/event-queue events) and the firmware sink (FreeRTOS task
/// resume/yield, `sim_trace_u32`). costar's `drain_trace_prefixed` appends these
/// sinks without merge-sorting, so a machine block legitimately steps from a high
/// World time (e.g. 2990500) back to a low firmware time (e.g. 0) at the sink
/// boundary. That boundary is NOT a violation.
///
/// So we track, per machine, both the current segment's start time and the last
/// time seen. A decrease that lands at or below the segment start is treated as a
/// new segment (a sink boundary) and resets the baseline. A decrease that stays
/// *above* the segment start is a genuine mid-segment regression and fails.
///
/// (A future trace v2 with explicit per-event stream/source identity would let
/// this check be exact rather than segment-inferred — see the plan's "Make Trace
/// v2 the Product Data Model" section.)
pub struct VirtualTimeMonotonic;

impl Invariant for VirtualTimeMonotonic {
    fn name(&self) -> &str {
        "VirtualTimeMonotonic"
    }
    fn check(&self, run: &ScenarioRun) -> InvariantResult {
        use std::collections::HashMap;
        // machine -> (segment_start, last_seen)
        let mut state: HashMap<String, (u64, u64)> = HashMap::new();
        let mut checked = 0usize;
        let mut segments = 0usize;
        for line in &run.trace {
            let Some((machine, time)) = parse_machine_time(line) else {
                continue;
            };
            checked += 1;
            match state.get_mut(machine) {
                None => {
                    state.insert(machine.to_string(), (time, time));
                    segments += 1;
                }
                Some((seg_start, last)) => {
                    if time >= *last {
                        // Non-decreasing within the current segment.
                        *last = time;
                    } else if time <= *seg_start {
                        // Reset at/below the segment start => new sink segment.
                        *seg_start = time;
                        *last = time;
                        segments += 1;
                    } else {
                        // Decrease that stays above the segment start: a genuine
                        // backwards step within a single monotonic stream.
                        return InvariantResult::fail(
                            self.name(),
                            format!(
                                "virtual time went backward for {machine} within a segment \
                                 (seg_start={seg_start}): {last} -> {time} in line: {line}"
                            ),
                        );
                    }
                }
            }
        }
        if checked == 0 {
            return InvariantResult::skipped(
                self.name(),
                "no parseable '[machine.N] <time> ...' lines in trace",
            );
        }
        InvariantResult::pass(
            self.name(),
            format!(
                "{checked} timed lines across {} machine(s) / {segments} segment(s), \
                 non-decreasing within every segment",
                state.len()
            ),
        )
    }
}

/// The child process must terminate cleanly: no timeout, no panic, exit 0.
pub struct ProcessTerminatedCleanly;

impl Invariant for ProcessTerminatedCleanly {
    fn name(&self) -> &str {
        "ProcessTerminatedCleanly"
    }
    fn check(&self, run: &ScenarioRun) -> InvariantResult {
        match run.status {
            RunStatus::Pass => InvariantResult::pass(self.name(), "exited 0, no timeout, no panic"),
            RunStatus::Timeout => InvariantResult::fail(
                self.name(),
                "process exceeded wall-clock timeout and was killed",
            ),
            RunStatus::Panic => InvariantResult::fail(
                self.name(),
                format!(
                    "process panicked (exit {:?}); stderr tail: {}",
                    run.exit_code,
                    run.stderr_tail.join(" | ")
                ),
            ),
            RunStatus::Fail => InvariantResult::fail(
                self.name(),
                format!(
                    "process exited non-zero (exit {:?}); stderr tail: {}",
                    run.exit_code,
                    run.stderr_tail.join(" | ")
                ),
            ),
        }
    }
}

/// The trace must contain at least one event line.
pub struct TraceNonEmpty;

impl Invariant for TraceNonEmpty {
    fn name(&self) -> &str {
        "TraceNonEmpty"
    }
    fn check(&self, run: &ScenarioRun) -> InvariantResult {
        if run.trace.is_empty() {
            InvariantResult::fail(self.name(), "trace has no event lines")
        } else {
            InvariantResult::pass(self.name(), format!("{} trace event lines", run.trace.len()))
        }
    }
}

// ─── Stub invariants (need richer trace data — future lanes) ────────────────

/// Emits a `Skipped` result carrying the reason the invariant isn't wired yet.
/// Kept as a distinct type per invariant so later lanes replace the `impl`
/// in-place without touching call sites.
macro_rules! stub_invariant {
    ($ty:ident, $name:literal, $todo:literal) => {
        #[doc = concat!("TODO stub for `", $name, "`: ", $todo)]
        pub struct $ty;
        impl Invariant for $ty {
            fn name(&self) -> &str {
                $name
            }
            fn check(&self, _run: &ScenarioRun) -> InvariantResult {
                InvariantResult::skipped($name, concat!("TODO: ", $todo))
            }
        }
    };
}

stub_invariant!(
    RunUntilNeverOvershoots,
    "RunUntilNeverOvershoots",
    "needs the run_until deadline and the final virtual time exposed in the trace"
);
stub_invariant!(
    SteppedEqualsContinuous,
    "SteppedEqualsContinuous",
    "needs a stepped-execution mode to compare against continuous run_until"
);
stub_invariant!(
    KeyframeRestoreReproduces,
    "KeyframeRestoreReproduces",
    "needs keyframe snapshot/restore support and a post-restore trace"
);
stub_invariant!(
    NoDroppedOrDuplicatedFrames,
    "NoDroppedOrDuplicatedFrames",
    "needs per-frame tx/rx identity + fault annotations in the trace"
);
stub_invariant!(
    GatewayForwardingPreservesCorrelationId,
    "GatewayForwardingPreservesCorrelationId",
    "needs correlation IDs and source/dest identity on forwarded frames"
);
stub_invariant!(
    InspectDevicesMatchesTrace,
    "InspectDevicesMatchesTrace",
    "needs an InspectDevices state dump to reconcile against trace-derived state"
);
stub_invariant!(
    ConcurrentEqualsSolo,
    "ConcurrentEqualsSolo",
    "needs the simfarm lane (N concurrent sessions in one server process)"
);

// ─── Driver ─────────────────────────────────────────────────────────────────

/// The full default invariant set: implemented checks first, then stubs.
pub fn default_invariants() -> Vec<Box<dyn Invariant>> {
    vec![
        Box::new(VirtualTimeMonotonic),
        Box::new(ProcessTerminatedCleanly),
        Box::new(TraceNonEmpty),
        Box::new(RunUntilNeverOvershoots),
        Box::new(SteppedEqualsContinuous),
        Box::new(KeyframeRestoreReproduces),
        Box::new(NoDroppedOrDuplicatedFrames),
        Box::new(GatewayForwardingPreservesCorrelationId),
        Box::new(InspectDevicesMatchesTrace),
        Box::new(ConcurrentEqualsSolo),
    ]
}

/// Evaluate every invariant against a run.
pub fn check_all(run: &ScenarioRun, invariants: &[Box<dyn Invariant>]) -> Vec<InvariantResult> {
    invariants.iter().map(|inv| inv.check(run)).collect()
}

/// Convenience: evaluate the default invariant set.
pub fn check_default(run: &ScenarioRun) -> Vec<InvariantResult> {
    check_all(run, &default_invariants())
}

/// True if any result is a hard `Fail` (skips do not count as failures).
pub fn any_failed(results: &[InvariantResult]) -> bool {
    results.iter().any(|r| r.is_fail())
}

/// Parse the `[machine.N]` prefix and the virtual-time value (second token).
/// Returns `(machine_prefix, time)` or `None` for non-trace lines.
fn parse_machine_time(line: &str) -> Option<(&str, u64)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("[machine.") {
        return None;
    }
    let mut it = trimmed.split_whitespace();
    let machine = it.next()?; // "[machine.N]"
    let time = it.next()?.parse::<u64>().ok()?;
    Some((machine, time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunStatus;
    use std::time::Duration;

    fn run_with(trace: &[&str], status: RunStatus) -> ScenarioRun {
        ScenarioRun {
            scenario: "synthetic".into(),
            status,
            exit_code: Some(if status == RunStatus::Pass { 0 } else { 1 }),
            trace: trace.iter().map(|s| s.to_string()).collect(),
            wall: Duration::from_millis(1),
            stdout_tail: Vec::new(),
            stderr_tail: Vec::new(),
        }
    }

    #[test]
    fn monotonic_passes_on_increasing_per_machine() {
        let run = run_with(
            &[
                "[machine.1]        10 can-rx id=0x0102",
                "[machine.1]        20 can-rx id=0x0102",
                "[machine.2]         5 task-created", // new machine resets — OK
                "[machine.2]        30 task-yield",
            ],
            RunStatus::Pass,
        );
        assert_eq!(VirtualTimeMonotonic.check(&run).status, CheckStatus::Pass);
    }

    #[test]
    fn monotonic_fails_on_midsegment_regression() {
        // A decrease that stays ABOVE the segment start (10) is a real
        // backwards step within one monotonic stream, not a sink boundary.
        let run = run_with(
            &[
                "[machine.1]        10 can-rx id=0x0102",
                "[machine.1]        30 can-rx id=0x0102",
                "[machine.1]        20 can-rx id=0x0102",
            ],
            RunStatus::Pass,
        );
        let r = VirtualTimeMonotonic.check(&run);
        assert_eq!(r.status, CheckStatus::Fail);
        assert!(r.message.contains("backward"));
    }

    #[test]
    fn monotonic_treats_reset_to_low_as_new_segment() {
        // The real microcar pattern: a high World-sink time then a firmware
        // sink that restarts near 0. Dropping to <= the segment start is a
        // legitimate sink boundary, not a violation.
        let run = run_with(
            &[
                "[machine.1]     10500 can-rx id=0x0102",
                "[machine.1]   2990500 can-rx id=0x0102",
                "[machine.1]         0 task-created id=1 name=\"gateway\"",
                "[machine.1]       500 task-yield",
            ],
            RunStatus::Pass,
        );
        assert_eq!(VirtualTimeMonotonic.check(&run).status, CheckStatus::Pass);
    }

    #[test]
    fn monotonic_skips_when_no_timed_lines() {
        let run = run_with(&["some non-trace text"], RunStatus::Pass);
        assert_eq!(VirtualTimeMonotonic.check(&run).status, CheckStatus::Skipped);
    }

    #[test]
    fn terminated_cleanly_maps_status() {
        assert_eq!(
            ProcessTerminatedCleanly.check(&run_with(&["x"], RunStatus::Pass)).status,
            CheckStatus::Pass
        );
        assert_eq!(
            ProcessTerminatedCleanly.check(&run_with(&["x"], RunStatus::Timeout)).status,
            CheckStatus::Fail
        );
        assert_eq!(
            ProcessTerminatedCleanly.check(&run_with(&["x"], RunStatus::Panic)).status,
            CheckStatus::Fail
        );
        assert_eq!(
            ProcessTerminatedCleanly.check(&run_with(&["x"], RunStatus::Fail)).status,
            CheckStatus::Fail
        );
    }

    #[test]
    fn nonempty_detects_empty() {
        assert_eq!(TraceNonEmpty.check(&run_with(&[], RunStatus::Pass)).status, CheckStatus::Fail);
        assert_eq!(TraceNonEmpty.check(&run_with(&["x"], RunStatus::Pass)).status, CheckStatus::Pass);
    }

    #[test]
    fn stubs_are_skipped() {
        let run = run_with(&["[machine.1] 10 x"], RunStatus::Pass);
        for inv in default_invariants() {
            let r = inv.check(&run);
            // The three implemented ones pass; everything else is skipped.
            let implemented = matches!(
                inv.name(),
                "VirtualTimeMonotonic" | "ProcessTerminatedCleanly" | "TraceNonEmpty"
            );
            if implemented {
                assert_eq!(r.status, CheckStatus::Pass, "{} should pass", inv.name());
            } else {
                assert_eq!(r.status, CheckStatus::Skipped, "{} should skip", inv.name());
            }
        }
    }

    #[test]
    fn any_failed_ignores_skips() {
        let results = vec![
            InvariantResult::pass("a", "ok"),
            InvariantResult::skipped("b", "later"),
        ];
        assert!(!any_failed(&results));
        let results = vec![InvariantResult::fail("c", "boom")];
        assert!(any_failed(&results));
    }

    #[test]
    fn parse_handles_padding() {
        assert_eq!(
            parse_machine_time("[machine.3]       120500 can-rx id=0x0500 len=7"),
            Some(("[machine.3]", 120500))
        );
        assert_eq!(parse_machine_time("PASS"), None);
        assert_eq!(parse_machine_time("=== x ==="), None);
    }
}
