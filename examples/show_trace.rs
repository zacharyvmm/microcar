//! Quick trace dumper — runs a scenario and prints the raw trace.
//! Usage: cargo run --example show_trace -- scenarios/<name>.toml

use sim_world::scenario::Scenario;
use microcar::MicrocarFirmware;
use microcar_plant::MicrocarPlant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: show_trace <scenario.toml>");
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

    if let Some(ms) = scenario.duration_ms {
        world.run_until(ms * 1000).unwrap();
    } else {
        world.run().unwrap();
    }

    for line in world.drain_all_traces() {
        println!("{}", line);
    }
    println!("\n=== Trace count: {} lines ===", world.drain_all_traces().len());
}
