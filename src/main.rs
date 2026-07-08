//! microcar — costar dogfood binary
//!
//! Runs ECU firmware on costar's fiber scheduler via the World event loop.
//! Usage: `microcar <scenario.toml>`
//!
//! Each machine with a `firmware` field gets a [`MicrocarFirmware`] instance
//! that exercises costar's fiber scheduler.
//!
//! ## Exit codes (stable — the dogfood harness and the `toml_zoo` lane depend
//! on these)
//!
//! | code | meaning                                                          |
//! |------|------------------------------------------------------------------|
//! | `0`  | scenario ran and its trace matched expectations (PASS)           |
//! | `1`  | scenario ran but failed at runtime (trace mismatch / sim error)  |
//! | `2`  | scenario could not be loaded or validated (structured config err)|
//!
//! Scenario/validation problems are reported as a single structured line on
//! stderr — `microcar: error [<kind>]: <detail>` — and the process exits `2`.
//! The binary never panics on malformed input: every fallible step is handled.

use std::process::ExitCode;

use microcar::validate::validate_scenario;
use microcar::MicrocarFirmware;
#[cfg(feature = "zephyr")]
use microcar::ZephyrDashboardFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::scenario::{Scenario, ScenarioError};

const EXIT_PASS: u8 = 0;
const EXIT_RUNTIME_FAIL: u8 = 1;
const EXIT_SCENARIO_ERROR: u8 = 2;

/// Stable machine-readable tag for a costar [`ScenarioError`].
fn scenario_error_kind(e: &ScenarioError) -> &'static str {
    match e {
        ScenarioError::Io(_) => "io",
        ScenarioError::Parse(_) => "parse",
        ScenarioError::Invalid(_) => "invalid",
        ScenarioError::Sim(_) => "sim",
        ScenarioError::TraceMismatch { .. } => "trace-mismatch",
    }
}

/// Print a structured scenario error to stderr and return the scenario-error
/// exit code.
fn fail_scenario(kind: &str, message: impl std::fmt::Display) -> ExitCode {
    eprintln!("microcar: error [{kind}]: {message}");
    ExitCode::from(EXIT_SCENARIO_ERROR)
}

fn main() -> ExitCode {
    let scenario_path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("microcar: error [usage]: usage: microcar <scenario.toml>");
            return ExitCode::from(EXIT_SCENARIO_ERROR);
        }
    };

    // ── Load + costar structural validation ─────────────────────
    let scenario = match Scenario::from_file(&scenario_path) {
        Ok(s) => s,
        Err(e) => return fail_scenario(scenario_error_kind(&e), e),
    };

    // ── microcar automotive-semantic validation ─────────────────
    if let Err(e) = validate_scenario(&scenario) {
        return fail_scenario(e.kind, e.message);
    }

    println!("=== {} ===\n", scenario.name);

    // ── Build world ─────────────────────────────────────────────
    let mut world = match scenario.build_world() {
        Ok(w) => w,
        Err(e) => return fail_scenario(scenario_error_kind(&e), e),
    };

    // ── Attach plant model ──────────────────────────────────────
    if scenario.plant.is_some() {
        let tick_ms = scenario
            .plant
            .as_ref()
            .and_then(|p| p.tick_ms)
            .unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        if let Err(e) = scenario.attach_plant_to(&mut world, Box::new(plant)) {
            return fail_scenario(scenario_error_kind(&e), e);
        }
    }

    // ── Attach firmware to each machine ─────────────────────────
    for m in &scenario.machine {
        if m.firmware.is_some() {
            if let Some(machine) = world.machine_mut(m.id) {
                let fw = m.firmware.as_deref().unwrap_or("");
                let is_zephyr = matches!(m.rtos, Some(sim_world::RtosBackend::Zephyr));
                if is_zephyr {
                    #[cfg(feature = "zephyr")]
                    {
                        machine.load_firmware(Box::new(ZephyrDashboardFirmware::new()));
                    }
                    #[cfg(not(feature = "zephyr"))]
                    {
                        eprintln!(
                            "warning: machine '{}' uses rtos=zephyr but \
                             the 'zephyr' feature is not enabled — \
                             falling back to FreeRTOS firmware",
                            m.name
                        );
                        machine.load_firmware(Box::new(MicrocarFirmware::with_firmware_path(
                            &m.name, fw,
                        )));
                    }
                } else {
                    machine
                        .load_firmware(Box::new(MicrocarFirmware::with_firmware_path(&m.name, fw)));
                }
            }
        }
    }

    // ── Schedule faults ─────────────────────────────────────────
    scenario.schedule_faults_to(&mut world);

    // ── Run simulation ──────────────────────────────────────────
    let run_result = if let Some(duration_ms) = scenario.duration_ms {
        // Convert ms to µs ticks (1 µs per tick).
        world.run_until(duration_ms * 1000)
    } else {
        world.run()
    };
    if let Err(e) = run_result {
        eprintln!("microcar: error [runtime]: simulation error: {e}");
        return ExitCode::from(EXIT_RUNTIME_FAIL);
    }

    // ── Check trace ─────────────────────────────────────────────
    let trace = world.drain_all_traces();
    let result = match scenario.check_trace(trace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("microcar: error [{}]: {e}", scenario_error_kind(&e));
            return ExitCode::from(EXIT_RUNTIME_FAIL);
        }
    };

    // Print trace events for golden trace capture.
    for event in &result.trace {
        println!("{event}");
    }

    if result.trace_match {
        println!("PASS");
        ExitCode::from(EXIT_PASS)
    } else {
        eprintln!("FAIL: trace mismatch");
        ExitCode::from(EXIT_RUNTIME_FAIL)
    }
}
