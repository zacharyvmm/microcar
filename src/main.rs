//! microcar — costar dogfood binary
//!
//! Runs ECU firmware on costar's fiber scheduler via the World event loop.
//! Usage: cargo run -- scenarios/<name>.toml
//!
//! Each machine with a `firmware` field gets a [`MicrocarFirmware`] instance
//! that exercises costar's fiber scheduler.

use sim_world::scenario::Scenario;
use microcar::MicrocarFirmware;
#[cfg(feature = "zephyr")]
use microcar::ZephyrDashboardFirmware;
use microcar_plant::MicrocarPlant;

fn main() {
    let scenario_path = std::env::args()
        .nth(1)
        .expect("usage: microcar <scenario.toml>");

    let scenario = Scenario::from_file(&scenario_path).unwrap();
    println!("=== {} ===\n", scenario.name);

    let mut world = scenario.build_world().unwrap();

    // ── Attach plant model ──────────────────────────────────────
    if let Some(ref plant_def) = scenario.plant {
        let tick_ms = plant_def.tick_ms.unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        scenario
            .attach_plant_to(&mut world, Box::new(plant))
            .unwrap();
    }

    // ── Attach firmware to each machine ─────────────────────────
    for m in &scenario.machine {
        if m.firmware.is_some() {
            if let Some(machine) = world.machine_mut(m.id) {
                let fw = m.firmware.as_deref().unwrap_or("");
                let rtos = m.rtos.as_deref().unwrap_or("freertos");
                if rtos == "zephyr" {
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
                        machine.load_firmware(Box::new(
                            MicrocarFirmware::with_firmware_path(&m.name, fw)
                        ));
                    }
                } else {
                    machine.load_firmware(Box::new(
                        MicrocarFirmware::with_firmware_path(&m.name, fw)
                    ));
                }
            }
        }
    }

    // ── Schedule faults ─────────────────────────────────────────
    scenario.schedule_faults_to(&mut world);

    // ── Run simulation ──────────────────────────────────────────
    if let Some(duration_ms) = scenario.duration_ms {
        // Convert ms to µs ticks (1 µs per tick).
        world.run_until(duration_ms * 1000).unwrap();
    } else {
        world.run().unwrap();
    }

    // ── Check trace ─────────────────────────────────────────────
    let trace = world.drain_all_traces();
    let result = scenario.check_trace(trace).unwrap();

    if result.trace_match {
        println!("PASS");
    } else {
        eprintln!("FAIL: trace mismatch");
        std::process::exit(1);
    }
}
