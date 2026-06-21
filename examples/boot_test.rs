//! Minimal FreeRTOS boot test — single machine, no World.

use sim_ffi::simulator::Simulator;
use sim_core::SimConfig;

extern "C" {
    fn microcar_boot();
    fn sim_scheduler_tick() -> u32;
}

fn main() {
    let mut sim = Simulator::new(SimConfig::default());
    println!("Simulator created");

    let _guard = sim.activate();
    println!("Simulator activated");

    unsafe { microcar_boot(); }
    println!("microcar_boot() called");

    // Scope the borrows
    {
        let global = sim.sim_global.borrow();
        println!("Tasks in SimGlobal: {}", global.tasks.len());
        if let Some(ref trace) = global.trace {
            println!("Firmware trace events (before tick): {}", trace.events().len());
        }
    }

    // Run scheduler ticks
    let mut total_ticks = 0;
    for i in 0..10 {
        let more = unsafe { sim_scheduler_tick() };
        total_ticks += 1;
        let global = sim.sim_global.borrow();
        let trace_len = global.trace.as_ref().map(|t| t.events().len()).unwrap_or(0);
        println!("Tick {}: more={}, tasks={}, trace={}", i, more, global.tasks.len(), trace_len);
        drop(global);
        if more == 0 { break; }
    }

    // Final trace dump
    let global = sim.sim_global.borrow();
    if let Some(ref trace) = global.trace {
        println!("\nFirmware trace ({} events, {} ticks):", trace.events().len(), total_ticks);
        for e in trace.events().iter().take(30) {
            println!("  {}", e);
        }
        if trace.events().len() > 30 {
            println!("  ... + {} more", trace.events().len() - 30);
        }
    } else {
        println!("No trace sink in SimGlobal!");
    }
}
