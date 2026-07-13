//! microcar — costar dogfood binary
//!
//! Runs ECU firmware on costar's fiber scheduler via the World event loop.
//! Usage: cargo run -- scenarios/<name>.toml
//!
//! Each machine with a `firmware` field gets a [`MicrocarFirmware`] instance
//! that exercises costar's fiber scheduler.

use std::process::ExitCode;

use microcar::MicrocarFirmware;
#[cfg(feature = "zephyr")]
use microcar::ZephyrDashboardFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::scenario::Scenario;

fn main() -> ExitCode {
    let scenario_path = std::env::args()
        .nth(1)
        .expect("usage: microcar <scenario.toml>");

    let scenario = Scenario::from_file(&scenario_path).unwrap();
    println!("=== {} ===\n", scenario.name);

    // Build world
    let mut world = scenario.build_world().unwrap_or_else(|e| {
        eprintln!("error building world: {e}");
        std::process::exit(1);
    });

    // Enable per-machine device ownership so CAN controller 0 and other
    // virtual devices are scoped per-machine (UNBLOCKING.md B1).
    world.enable_owned_device_banks();
    // Attach plant model
    if let Some(ref plant_def) = scenario.plant {
        let tick_ms = plant_def.tick_ms.unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        world.set_plant(Box::new(plant), tick_ms);
    }

    // Attach firmware to each machine
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
                            m.name.clone(),
                            fw.to_string(),
                        )));
                    }
                } else {
                    machine.load_firmware(Box::new(MicrocarFirmware::with_firmware_path(
                        m.name.clone(),
                        fw.to_string(),
                    )));
                }
            }
        }
    }

    // Schedule faults
    scenario.schedule_faults_to(&mut world);

    // Run simulation
    if let Some(duration_ms) = scenario.duration_ms {
        world.run_until(duration_ms * 1000).unwrap();
    } else {
        world.run().unwrap();
    }

    // Check trace
    let trace = world.drain_all_traces();
    let result = scenario.check_trace(trace).unwrap();

    // Print trace events for golden trace capture.
    for event in &result.trace {
        println!("{}", event);
    }

    if result.trace_match {
        println!("PASS");
        ExitCode::SUCCESS
    } else {
        eprintln!("FAIL: trace mismatch");
        ExitCode::FAILURE
    }
}
