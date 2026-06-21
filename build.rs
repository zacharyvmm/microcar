//! Build script for microcar.
//!
//! Compiles the C firmware files (ECU source + common library + coordinator)
//! and links them into the microcar binary.  FreeRTOS itself is compiled by
//! sim-freertos-port; we only need the include paths.

use std::path::PathBuf;

fn main() {
    // ── Paths relative to this crate's root ──────────────────────────
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    let costar_root = manifest_dir.join("../costar");
    let freertos_port = costar_root.join("crates/sim-freertos-port");
    let sim_ffi_include = costar_root.join("crates/sim-ffi/include");

    // ── Re-run triggers ──────────────────────────────────────────────
    println!("cargo:rerun-if-changed=common/include/");
    println!("cargo:rerun-if-changed=common/src/");
    println!("cargo:rerun-if-changed=firmware/bms_ecu/src/");
    println!("cargo:rerun-if-changed=firmware/dashboard_ecu/src/");
    println!("cargo:rerun-if-changed=firmware/dashboard_ecu_zephyr/src/");
    println!("cargo:rerun-if-changed=firmware/gateway_ecu/src/");
    println!("cargo:rerun-if-changed=firmware/powertrain_ecu/src/");
    println!("cargo:rerun-if-changed=firmware/microcar_coordinator.c");

    // Also re-run if the FreeRTOS config or sim ABI headers change.
    println!("cargo:rerun-if-changed={}", freertos_port.join("c/FreeRTOSConfig.h").display());
    println!("cargo:rerun-if-changed={}", freertos_port.join("c/portmacro.h").display());
    println!("cargo:rerun-if-changed={}", sim_ffi_include.join("sim_abi.h").display());

    let mut build = cc::Build::new();

    // ── Common library ───────────────────────────────────────────────
    build.file("common/src/microcar_protocol.c");

    // ── BMS ECU ──────────────────────────────────────────────────────
    build
        .file("firmware/bms_ecu/src/main.c")
        .file("firmware/bms_ecu/src/bms_state.c")
        .file("firmware/bms_ecu/src/bms_limits.c");

    // ── Dashboard ECU (FreeRTOS) ─────────────────────────────────────
    build
        .file("firmware/dashboard_ecu/src/main.c")
        .file("firmware/dashboard_ecu/src/dashboard_state.c")
        .file("firmware/dashboard_ecu/src/warning_display.c");

    // ── Gateway ECU ──────────────────────────────────────────────────
    build
        .file("firmware/gateway_ecu/src/main.c")
        .file("firmware/gateway_ecu/src/gateway_state.c")
        .file("firmware/gateway_ecu/src/heartbeat_monitor.c")
        .file("firmware/gateway_ecu/src/fault_manager.c");

    // ── Powertrain ECU ───────────────────────────────────────────────
    build
        .file("firmware/powertrain_ecu/src/main.c")
        .file("firmware/powertrain_ecu/src/torque_controller.c")
        .file("firmware/powertrain_ecu/src/safety_rules.c")
        .file("firmware/powertrain_ecu/src/watchdog_task.c");

    // ── Coordinator (boot entry) ─────────────────────────────────────
    build.file("firmware/microcar_coordinator.c");

    // ── Zephyr dashboard (only when feature enabled) ─────────────────
    // The Zephyr dashboard uses <zephyr/kernel.h> which is only
    // available when the Zephyr SDK or sim-zephyr-port is present.
    #[cfg(feature = "zephyr")]
    {
        build.file("firmware/dashboard_ecu_zephyr/src/main.c")
             .file("firmware/dashboard_ecu_zephyr/src/dashboard_state.c")
             .file("firmware/dashboard_ecu_zephyr/src/warning_display.c");
        // Zephyr include paths would be added here when the feature is active.
    }

    // ── Include paths ────────────────────────────────────────────────
    build
        // FreeRTOS kernel headers
        .include(freertos_port.join("FreeRTOS-Kernel/include"))
        // FreeRTOS port config (portmacro.h, FreeRTOSConfig.h)
        .include(freertos_port.join("c"))
        // sim-ffi ABI (sim_abi.h)
        .include(&sim_ffi_include)
        // microcar common headers
        .include("common/include")
        // ECU-specific headers (find headers next to source)
        .include("firmware/bms_ecu/src")
        .include("firmware/dashboard_ecu/src")
        .include("firmware/dashboard_ecu_zephyr/src")
        .include("firmware/gateway_ecu/src")
        .include("firmware/powertrain_ecu/src");

    // ── Defines ──────────────────────────────────────────────────────
    build
        .define("SIMULATION_HOST_MODE", Some("1"))
        .define("FREERTOS_PORT_SIM", Some("1"));

    // ── Compiler flags ──────────────────────────────────────────────
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        build.flag_if_supported("-Wall");
        build.flag_if_supported("-Wextra");
        build.flag_if_supported("-Wno-unused-parameter");
        build.flag_if_supported("-Wno-sign-compare");
        build.flag_if_supported("-Wno-missing-field-initializers");
    }

    // ── Compile ──────────────────────────────────────────────────────
    build.compile("embedded_microcar_payload");
}
