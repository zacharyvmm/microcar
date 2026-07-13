//! microcar — costar dogfood binary
//!
//! Runs ECU firmware on costar's fiber scheduler via the World event loop.
//! Usage: cargo run -- scenarios/<name>.toml
//!
//! Exit codes:
//!   0 — pass (trace matched, all checks passed)
//!   1 — runtime/check failure
//!   2 — scenario/validation error (malformed TOML, unknown firmware, etc.)

use std::process::ExitCode;

use microcar::MicrocarFirmware;
#[cfg(feature = "zephyr")]
use microcar::ZephyrDashboardFirmware;
use microcar::validate;
use microcar_plant::MicrocarPlant;
use sim_world::scenario::Scenario;

fn main() -> ExitCode {
    let scenario_path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("microcar: error [usage]: usage: microcar <scenario.toml>");
            return ExitCode::from(2);
        }
    };

    // Parse scenario — structured error on malformed TOML.
    let scenario = match Scenario::from_file(&scenario_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("microcar: error [parse]: {e}");
            return ExitCode::from(2);
        }
    };

    // ── Automotive-semantic validation (microcar-specific rules) ──
    if let Err(e) = validate::validate_scenario(&scenario) {
        eprintln!("microcar: error [validate]: {e}");
        return ExitCode::from(2);
    }

    println!("=== {} ===\n", scenario.name);

    // Build world.
    let mut world = match scenario.build_world() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("microcar: error [build]: {e}");
            return ExitCode::from(1);
        }
    };

    // Enable per-machine device ownership so CAN controller 0 and other
    // virtual devices are scoped per-machine (UNBLOCKING.md B1).
    world.enable_owned_device_banks();

    // Attach plant model.
    if let Some(ref plant_def) = scenario.plant {
        let tick_ms = plant_def.tick_ms.unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        world.set_plant(Box::new(plant), tick_ms);
    }

    // Attach firmware to each machine.
    for m in &scenario.machine {
        if m.firmware.is_some() {
            if let Some(machine) = world.machine_mut(m.id) {
                let fw = m.firmware.as_deref().unwrap_or("");
                let is_zephyr = matches!(m.rtos, Some(sim_world::RtosBackend::Zephyr));
                if is_zephyr {
                    #[cfg(feature = "zephyr")]
                    {
                        machine.set_firmware_factory(std::sync::Arc::new(|| {
                            Box::new(ZephyrDashboardFirmware::new())
                        }));
                        machine.load_firmware(Box::new(ZephyrDashboardFirmware::new()));
                    }
                    #[cfg(not(feature = "zephyr"))]
                    {
                        eprintln!(
                            "microcar: warning: machine '{}' uses rtos=zephyr but \
                             the 'zephyr' feature is not enabled — \
                             falling back to FreeRTOS firmware",
                            m.name
                        );
                        machine.load_firmware(Box::new(MicrocarFirmware::with_firmware_path(
                            m.name.clone(),
                            fw.to_string(),
                        )));
                    }
                } else {
                    // Set the firmware factory so restart can reconstruct firmware.
                    let firmware_path = fw.to_string();
                    let name = m.name.clone();
                    machine.set_firmware_factory(std::sync::Arc::new(move || {
                        Box::new(MicrocarFirmware::with_firmware_path(
                            name.clone(),
                            firmware_path.clone(),
                        ))
                    }));
                    machine.load_firmware(Box::new(MicrocarFirmware::with_firmware_path(
                        m.name.clone(),
                        fw.to_string(),
                    )));
                }
            }
        }
    }

    // Schedule faults.
    scenario.schedule_faults_to(&mut world);

    // Run simulation.
    let run_result = if let Some(duration_ms) = scenario.duration_ms {
        world.run_until(duration_ms * 1000)
    } else {
        world.run()
    };

    if let Err(e) = run_result {
        eprintln!("microcar: error [runtime]: {e}");
        return ExitCode::from(1);
    }

    // Check trace.
    let trace = world.drain_all_traces();
    let result = match scenario.check_trace(trace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("microcar: error [trace]: {e}");
            return ExitCode::from(1);
        }
    };

    // Print trace events for golden trace capture.
    for event in &result.trace {
        println!("{}", event);
    }

    if result.trace_match {
        println!("PASS");
        ExitCode::SUCCESS
    } else {
        eprintln!("microcar: error [check]: trace mismatch");
        ExitCode::from(1)
    }
}
