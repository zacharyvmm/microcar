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
use std::sync::Arc;

use microcar::validate::validate_scenario;
use microcar::MicrocarFirmware;
#[cfg(feature = "zephyr")]
use microcar::ZephyrDashboardFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::firmware::FirmwareFactory;
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

/// Drive the World one event at a time via [`sim_world::World::step`], honoring
/// the same deadline semantics as `run_until` (or running to idle when
/// `deadline` is `None`). Trace-identical to the continuous path by
/// construction — `run`/`run_until` delegate to the same `step()`.
fn step_run(
    world: &mut sim_world::World,
    deadline: Option<sim_core::Tick>,
) -> Result<(), sim_core::SimError> {
    match deadline {
        Some(d) => {
            while world.now < d {
                match world.next_global_event_time() {
                    Some(t) if t <= d => match world.step()? {
                        sim_world::StepOutcome::Advanced(_) => {}
                        sim_world::StepOutcome::Done => break,
                    },
                    _ => break,
                }
            }
        }
        None => while let sim_world::StepOutcome::Advanced(_) = world.step()? {},
    }
    Ok(())
}

fn main() -> ExitCode {
    // ── Parse args: <scenario.toml> [--trace-v2 <path>] [--step] ─
    let mut scenario_path: Option<String> = None;
    let mut trace_v2_path: Option<String> = None;
    let mut step_mode = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--trace-v2" => match args.next() {
                Some(p) => trace_v2_path = Some(p),
                None => {
                    eprintln!("microcar: error [usage]: --trace-v2 needs a path");
                    return ExitCode::from(EXIT_SCENARIO_ERROR);
                }
            },
            "--step" => step_mode = true,
            other if other.starts_with("--") => {
                eprintln!("microcar: error [usage]: unknown flag '{other}'");
                return ExitCode::from(EXIT_SCENARIO_ERROR);
            }
            other if scenario_path.is_none() => scenario_path = Some(other.to_string()),
            _ => {}
        }
    }
    let scenario_path = match scenario_path {
        Some(p) => p,
        None => {
            eprintln!(
                "microcar: error [usage]: usage: microcar <scenario.toml> [--trace-v2 <path>] [--step]"
            );
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

    // Enable the opt-in Trace v2 sink if requested (additive; does not change
    // the default human trace output).
    if trace_v2_path.is_some() {
        world.enable_trace_v2();
    }

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
    // Uses `load_firmware_from_factory` so a later restart (P1, B3)
    // can recreate the original firmware and run its boot path —
    // instead of leaving a bare machine after reboot.
    for m in &scenario.machine {
        if m.firmware.is_some() {
            if let Some(machine) = world.machine_mut(m.id) {
                let fw = m.firmware.as_deref().unwrap_or("");
                let is_zephyr = matches!(m.rtos, Some(sim_world::RtosBackend::Zephyr));
                if is_zephyr {
                    #[cfg(feature = "zephyr")]
                    {
                        let factory: FirmwareFactory =
                            Arc::new(|| Box::new(ZephyrDashboardFirmware::new()));
                        machine.load_firmware_from_factory(factory);
                    }
                    #[cfg(not(feature = "zephyr"))]
                    {
                        eprintln!(
                            "warning: machine '{}' uses rtos=zephyr but \
                             the 'zephyr' feature is not enabled — \
                             falling back to FreeRTOS firmware",
                            m.name
                        );
                        let name = m.name.clone();
                        let fw_path = fw.to_string();
                        let factory: FirmwareFactory = Arc::new(move || {
                            Box::new(MicrocarFirmware::with_firmware_path(
                                name.clone(),
                                fw_path.clone(),
                            ))
                        });
                        machine.load_firmware_from_factory(factory);
                    }
                } else {
                    let name = m.name.clone();
                    let fw_path = fw.to_string();
                    let factory: FirmwareFactory = Arc::new(move || {
                        Box::new(MicrocarFirmware::with_firmware_path(
                            name.clone(),
                            fw_path.clone(),
                        ))
                    });
                    machine.load_firmware_from_factory(factory);
                }
            }
        }
    }

    // ── Schedule faults ─────────────────────────────────────────
    scenario.schedule_faults_to(&mut world);

    // ── Run simulation ──────────────────────────────────────────
    // `--step` drives the run one event at a time via World::step(), which is
    // trace-identical to a continuous run/run_until by construction (it is what
    // they delegate to). This exercises the stepping primitive through the
    // product binary and is what the dogfood debug_gym lane compares against a
    // continuous run.
    let deadline = scenario.duration_ms.map(|ms| ms * 1000);
    let run_result = if step_mode {
        step_run(&mut world, deadline)
    } else if let Some(d) = deadline {
        // Convert ms to µs ticks (1 µs per tick).
        world.run_until(d)
    } else {
        world.run()
    };
    if let Err(e) = run_result {
        eprintln!("microcar: error [runtime]: simulation error: {e}");
        return ExitCode::from(EXIT_RUNTIME_FAIL);
    }

    // ── Write Trace v2 JSONL if requested (best-effort; never changes the
    //    exit code or the default trace output) ───────────────────
    if let Some(path) = &trace_v2_path {
        let jsonl = world.trace_v2_jsonl();
        if let Err(e) = std::fs::write(path, jsonl) {
            eprintln!("microcar: warning: failed to write trace v2 to {path}: {e}");
        }
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
