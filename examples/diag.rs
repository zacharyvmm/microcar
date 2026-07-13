//! Quick diagnostic — shows firmware trace + scheduler tick count.
//! Usage: cargo run --example diag -- scenarios/<name>.toml
//!
//! NOTE: This example needs updating for the latest MicrocarFirmware API.
//! Re-enable after B2 C-global migration completes.

use microcar::MicrocarFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::scenario::Scenario;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: diag <scenario.toml>");
    let scenario = Scenario::from_file(&path).unwrap();
    let mut world = scenario.build_world().unwrap();

    if let Some(ref plant_def) = scenario.plant {
        let tick_ms = plant_def.tick_ms.unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        world.set_plant(Box::new(plant), tick_ms);
    }

    for m in &scenario.machine {
        if m.firmware.is_some() {
            if let Some(machine) = world.machine_mut(m.id) {
                let fw = m.firmware.as_deref().unwrap_or("");
                machine.load_firmware(Box::new(MicrocarFirmware::with_firmware_path(
                    m.name.clone(),
                    fw.to_string(),
                )));
            }
        }
    }

    scenario.schedule_faults_to(&mut world);

    if let Some(ms) = scenario.duration_ms {
        world.run_until(ms * 1000).unwrap();
    } else {
        world.run().unwrap();
    }

    let trace = world.drain_all_traces();
    for line in &trace {
        println!("{}", line);
    }
}
