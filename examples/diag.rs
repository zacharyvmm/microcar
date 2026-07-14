//! Quick diagnostic — prints each machine's accumulated trace line count.
//! Usage: cargo run --example diag -- scenarios/<name>.toml

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
        scenario
            .attach_plant_to(&mut world, Box::new(plant))
            .unwrap();
    }

    for m in &scenario.machine {
        if m.firmware.is_some() {
            if let Some(machine) = world.machine_mut(m.id) {
                machine.load_firmware(Box::new(MicrocarFirmware::new(&m.name)));
            }
        }
    }

    scenario.schedule_faults_to(&mut world);

    // Run and also check firmware trace
    if let Some(ms) = scenario.duration_ms {
        world.run_until(ms * 1000).unwrap();
    } else {
        world.run().unwrap();
    }

    // Print each machine's accumulated trace line count via the public API.
    for id in world.machine_ids().collect::<Vec<_>>() {
        if let Some(m) = world.machine(id) {
            let lines = m.drain_trace_prefixed();
            println!("[machine.{}] {} trace lines", id, lines.len());
        }
    }
}
