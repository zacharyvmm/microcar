//! microcar — costar dogfood binary
//!
//! Runs ECU firmware on costar's fiber scheduler via the World event loop.
//! Usage: cargo run -- scenarios/<name>.toml
//!
//! Each machine with a `firmware` field gets a [`MicrocarFirmware`] instance
//! that exercises costar's fiber scheduler.

use std::process::ExitCode;
use std::sync::Arc;

use microcar::validate::validate_scenario;
use microcar::MicrocarFirmware;
#[cfg(feature = "zephyr")]
use microcar::ZephyrDashboardFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::scenario::Scenario;
use sim_world::firmware::FirmwareFactory;
use sim_world::scenario::{Scenario, ScenarioError};

fn main() {
    let scenario_path = std::env::args()
        .nth(1)
        .expect("usage: microcar <scenario.toml>");

    let scenario = Scenario::from_file(&scenario_path).unwrap();
    println!("=== {} ===\n", scenario.name);

    // ── Build world ─────────────────────────────────────────────
    let mut world = match scenario.build_world() {
        Ok(w) => w,
        Err(e) => return fail_scenario(scenario_error_kind(&e), e),
    };

    // ── B1: per-machine device ownership ───────────────────────
    // Every machine gets its own DeviceBank so CAN controller 0 and other
    // virtual devices are scoped per-machine rather than shared across the
    // process (UNBLOCKING.md §B1).  This is the gate for reliable firmware
    // CAN RX/TX and the basis for real diagnostics/charging/OTA over CAN.
    // Enabled BEFORE firmware attachment so the lazy CAN controller-0
    // provisioning is visible during Firmware::init.
    world.enable_owned_device_banks();

    // Enable the opt-in Trace v2 sink if requested (additive; does not change
    // the default human trace output).
    if trace_v2_path.is_some() {
        world.enable_trace_v2();
    }

    // ── Attach plant model ──────────────────────────────────────
    if let Some(ref plant_def) = scenario.plant {
        let tick_ms = plant_def.tick_ms.unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        scenario
            .attach_plant_to(&mut world, Box::new(plant))
            .unwrap();
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
    if let Some(duration_ms) = scenario.duration_ms {
        // Convert ms to µs ticks (1 µs per tick).
        world.run_until(duration_ms * 1000).unwrap();
    } else {
        world.run().unwrap();
    }

    // ── Check trace ─────────────────────────────────────────────
    let trace = world.drain_all_traces();
    let result = scenario.check_trace(trace).unwrap();

    // Print trace events for golden trace capture.
    for event in &result.trace {
        println!("{}", event);
    }

    if result.trace_match {
        println!("PASS");
    } else {
        eprintln!("FAIL: trace mismatch");
        std::process::exit(1);
    }
}
