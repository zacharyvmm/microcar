//! Performance benchmarks for the microcar simulation.
//!
//! Measures:
//!   - Events per second of virtual time
//!   - Wall-clock seconds per 1M virtual ticks
//!   - Memory per machine (approximate via RSS before/after)
//!
//! Usage:
//!   cargo run --bin microcar -- ../microcar/scenarios/bench_scenario.toml
//!
//! Or for dedicated benchmarks:
//!   cargo run --example bench -- [scenario_path] [--runs N]

use std::time::Instant;

use sim_world::Scenario;

fn main() {
    let _args: Vec<String> = std::env::args().collect();

    // Default scenario paths
    let default_scenarios = vec![
        "fleet_of_8",
        "fleet_of_16",
        "normal_drive_cycle",
    ];

    let microcar_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let scenarios_dir = microcar_dir.join("scenarios");

    println!("=== microcar Performance Benchmarks ===\n");

    // ── Memory baseline ──────────────────────────────────────
    let rss_before = get_rss_kb();
    println!("Memory (RSS) before: {} KB", rss_before);

    // ── Run each scenario and collect metrics ─────────────────
    for stem in &default_scenarios {
        let path = scenarios_dir.join(format!("{}.toml", stem));
        if !path.exists() {
            println!("  SKIP {}: file not found", stem);
            continue;
        }

        let path_str = path.to_string_lossy();
        println!("\n--- {} ---", stem);

        let scenario = match Scenario::from_file(&path_str) {
            Ok(s) => s,
            Err(e) => {
                println!("  ERROR loading scenario: {}", e);
                continue;
            }
        };

        let num_machines = scenario.machine.len();
        let duration_ms = scenario.duration_ms.unwrap_or(0);
        let virtual_ticks = duration_ms * 1000;

        println!(
            "  Machines: {}, Virtual time: {} ms ({} ticks)",
            num_machines, duration_ms, virtual_ticks
        );

        // Build the world (timed)
        let build_start = Instant::now();
        let mut world = match scenario.build_world() {
            Ok(w) => w,
            Err(e) => {
                println!("  ERROR building world: {}", e);
                continue;
            }
        };
        let build_time = build_start.elapsed();
        println!("  Build time: {:?}", build_time);

        // Attach plant if configured
        if let Some(ref plant_def) = scenario.plant {
            let tick_ms = plant_def.tick_ms.unwrap_or(10) as u32;
            let plant = microcar_plant::MicrocarPlant::new(tick_ms);
            scenario
                .attach_plant_to(&mut world, Box::new(plant))
                .expect("failed to attach plant");
        }

        // Schedule faults
        scenario.schedule_faults_to(&mut world);

        // Run simulation (timed)
        let sim_start = Instant::now();
        if let Some(ms) = scenario.duration_ms {
            world.run_until(ms * 1000).expect("simulation failed");
        } else {
            world.run().expect("simulation failed");
        }
        let sim_time = sim_start.elapsed();

        // Collect traces
        let trace = world.drain_all_traces();
        let trace_events = trace.len();

        println!("  Simulation wall time: {:?}", sim_time);
        println!("  Trace events: {}", trace_events);

        if virtual_ticks > 0 {
            let events_per_second_vt =
                (trace_events as f64 / duration_ms as f64) * 1000.0;
            let wall_sec_per_1m_ticks =
                (sim_time.as_secs_f64() / virtual_ticks as f64) * 1_000_000.0;
            let virtual_tick_rate =
                virtual_ticks as f64 / sim_time.as_secs_f64();

            println!("  Events/sec (virtual): {:.2}", events_per_second_vt);
            println!("  Wall sec / 1M virtual ticks: {:.6}", wall_sec_per_1m_ticks);
            println!("  Virtual tick rate: {:.0} ticks/sec", virtual_tick_rate);
        }
    }

    // ── Memory after ─────────────────────────────────────────
    let rss_after = get_rss_kb();
    println!("\nMemory (RSS) after: {} KB", rss_after);
    if rss_after > rss_before {
        println!(
            "Delta: +{} KB ({:.1} KB/machine est.)",
            rss_after - rss_before,
            (rss_after - rss_before) as f64 / 4.0
        );
    }

    println!("\n=== Benchmarks Complete ===");
}

/// Get current process RSS in kilobytes from /proc/self/statm (Linux)
/// or via `ps` on macOS.
fn get_rss_kb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<u64>() {
                    // statm reports in pages; assume 4KB pages
                    return pages * 4;
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Use `ps -o rss= -p $$` to get RSS in KB
        if let Ok(output) = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
        {
            if output.status.success() {
                if let Ok(rss_str) = String::from_utf8(output.stdout) {
                    if let Ok(rss) = rss_str.trim().parse::<u64>() {
                        return rss; // already in KB on macOS
                    }
                }
            }
        }
    }

    // Fallback
    0
}
