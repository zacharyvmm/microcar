//! Quick diagnostic — shows firmware trace + scheduler tick count.
//! Usage: cargo run --example diag -- scenarios/<name>.toml

use sim_world::scenario::Scenario;
use microcar::MicrocarFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::Machine;

extern "C" {
    fn sim_scheduler_tick() -> u32;
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: diag <scenario.toml>");
    let scenario = Scenario::from_file(&path).unwrap();
    let mut world = scenario.build_world().unwrap();

    if let Some(ref plant_def) = scenario.plant {
        let tick_ms = plant_def.tick_ms.unwrap_or(10);
        let plant = MicrocarPlant::new(tick_ms as u32);
        scenario.attach_plant_to(&mut world, Box::new(plant)).unwrap();
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

    // Check each machine's firmware trace
    for m in world.machines() {
        let fw_count = m.simulator().sim_global.borrow()
            .trace.as_ref()
            .map(|t| t.events().len())
            .unwrap_or(0);
        println!("[machine.{}] World: {} events, Firmware: {} events",
            m.id, m.trace().len(), fw_count);
    }
}
