//! # microcar-dogfood
//!
//! Foundation for the microcar/costar dogfood harness (see
//! `docs/costar_microcar_dogfood_plan.md`, "Dogfood Harness Foundation").
//!
//! It runs costar scenarios through the `microcar` binary, normalizes and hashes
//! their traces, checks invariants, enforces wall-clock timeouts, verifies
//! solo-vs-repeat determinism, and emits JSON summaries for CI.
//!
//! ## Why subprocess-driven?
//!
//! The simulator's `World` is `!Send`, so it can't be handed to a timeout thread
//! for in-process cancellation. The harness therefore spawns the `microcar`
//! binary as a child process ([`runner`]). That buys three things the plan
//! requires: real wall-clock timeout enforcement (kill the child), panic
//! isolation (a child panic can't unwind us), and zero coupling to the `sim-*`
//! crates — this crate is **std-only, no external dependencies**.
//!
//! ## Module map
//!
//! * [`runner`] — spawn microcar, capture trace, enforce timeout.
//! * [`trace_hash`] — conservative normalization + stable FNV-1a hashing.
//! * [`invariants`] — [`invariants::Invariant`] trait, implemented checks, and
//!   TODO stubs for the invariants that need richer trace data.
//! * [`determinism`] — run N times, assert all trace hashes match.
//! * [`summary`] / [`json`] — hand-rolled JSON summary emitter.

pub mod charging;
pub mod debug_gym;
pub mod determinism;
pub mod diagnostics;
pub mod invariants;
pub mod json;
pub mod runner;
pub mod simfarm;
pub mod summary;
pub mod toml_zoo;
pub mod topology;
pub mod trace_hash;

pub use charging::{
    run_charging, run_charging_scenario, ChargingCheck, ChargingReport, ChargingScenarioResult,
    DEFAULT_CHARGING_DIR,
};
pub use debug_gym::{run_debug_gym, DebugGymReport, DebugGymScenarioResult, DEFAULT_SCENARIOS};
pub use determinism::{check_solo_vs_repeat, DeterminismReport};
pub use diagnostics::{
    run_diagnostics, run_diagnostics_scenario, DiagnosticsCheck, DiagnosticsReport,
    DiagnosticsScenarioResult, DEFAULT_DIAGNOSTICS_DIR,
};
pub use invariants::{
    any_failed, check_all, check_default, default_invariants, CheckStatus, Invariant,
    InvariantResult,
};
pub use runner::{run_scenario, RunStatus, ScenarioRun};
pub use simfarm::{
    run_churn, run_panic_isolation, run_simfarm, ChurnReport, PanicIsolationReport, RunHash,
    SimfarmReport,
};
pub use summary::{build_summary, write_summary, Summary, HARNESS_VERSION};
pub use toml_zoo::{
    discover_cases, run_sibling_isolation, run_toml_zoo, CaseResult, SiblingIsolation, TomlZooCase,
    TomlZooReport, DEFAULT_CORPUS_DIR,
};
pub use topology::{
    run_topology, run_topology_scenario, Probe, ProbeResult, TopologyReport,
    TopologyScenarioResult, DEFAULT_TOPOLOGY_DIR,
};
pub use trace_hash::{normalize_trace, normalized_hash, trace_hash};
