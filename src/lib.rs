//! microcar — costar dogfood firmware bridge
//!
//! [`MicrocarFirmware`] implements the [`Firmware`](sim_world::firmware::Firmware)
//! trait.  Each microcar ECU gets its own firmware instance.
//!
//! In `init()`, the firmware activates the machine's SimGlobal and calls
//! the appropriate per-ECU C boot function.  In `step()`, it
//! calls `sim_scheduler_tick()` to advance the FreeRTOS scheduler by one
//! cycle on the machine's fiber pool.
//!
//! ## ECU selection
//!
//! Firmware is selected via the scenario's `firmware` field (e.g.
//! `firmware = "firmware/gateway_ecu"`).  The firmware path determines
//! which ECU boot function is called:
//!
//! | firmware field                | boot function          |
//! |-------------------------------|------------------------|
//! | `firmware/gateway_ecu`        | `microcar_boot_gateway` |
//! | `firmware/powertrain_ecu`     | `microcar_boot_powertrain` |
//! | `firmware/bms_ecu`            | `microcar_boot_bms` |
//! | `firmware/dashboard_ecu`      | `microcar_boot_dashboard` |
//!
//! If the firmware field does not contain a recognised ECU name the
//! machine name is used as a fallback (backwards-compatible).
//!
//! ## Zephyr support (feature = "zephyr")
//!
//! When the `zephyr` feature is enabled, [`ZephyrDashboardFirmware`] is
//! available for machines with `rtos = "zephyr"`.  It boots the Zephyr
//! dashboard ECU firmware and advances the scheduler via
//! `sim_zephyr_scheduler_tick()`.

use sim_world::firmware::Firmware;
use sim_world::Machine;
use sim_core::Tick;

// C ABI functions from the compiled firmware / sim-ffi.
extern "C" {
    fn microcar_boot();
    fn microcar_boot_gateway();
    fn microcar_boot_powertrain();
    fn microcar_boot_bms();
    fn microcar_boot_dashboard();
    fn microcar_boot_priority_inversion();
    fn microcar_boot_lifecycle_stress();
    fn microcar_boot_test_fiber();
    fn sim_scheduler_tick() -> u32;
}

// Zephyr C ABI functions (available when feature = "zephyr" is enabled).
// These link to the Zephyr dashboard firmware compiled by build.rs and
// the new `sim_zephyr_scheduler_tick()` in sim-ffi.
#[cfg(feature = "zephyr")]
extern "C" {
    fn microcar_boot_dashboard_zephyr();
    fn sim_zephyr_scheduler_tick() -> u32;
}

/// Firmware for a single microcar FreeRTOS ECU.
pub struct MicrocarFirmware {
    /// Machine name (e.g. "gateway").
    pub name: String,
    /// Firmware path from the scenario (e.g. "firmware/gateway_ecu").
    pub firmware_path: Option<String>,
    booted: bool,
}

impl MicrocarFirmware {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            firmware_path: None,
            booted: false,
        }
    }

    pub fn with_firmware_path(name: impl Into<String>, firmware_path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            firmware_path: Some(firmware_path.into()),
            booted: false,
        }
    }

    fn ecu_type(&self) -> &str {
        if let Some(ref path) = self.firmware_path {
            if path.contains("priority_inversion") {
                return "priority_inversion";
            }
            if path.contains("lifecycle_stress") {
                return "lifecycle_stress";
            }
            if path.contains("gateway") {
                return "gateway";
            }
            if path.contains("powertrain") {
                return "powertrain";
            }
            if path.contains("bms") {
                return "bms";
            }
            if path.contains("dashboard") {
                return "dashboard";
            }
        }
        &self.name
    }
}

impl Firmware for MicrocarFirmware {
    fn init(&mut self, machine: &mut Machine) {
        let _guard = machine.activate();

        let ecu = self.ecu_type();
        unsafe {
            if ecu.starts_with("priority_inversion") {
                microcar_boot_priority_inversion();
            } else if ecu.starts_with("lifecycle_stress") {
                microcar_boot_lifecycle_stress();
            } else if ecu.starts_with("test_fiber") {
                microcar_boot_test_fiber();
            } else if ecu.starts_with("gateway") {
                microcar_boot_gateway();
            } else if ecu.starts_with("powertrain") {
                microcar_boot_powertrain();
            } else if ecu.starts_with("bms") {
                microcar_boot_bms();
            } else if ecu.starts_with("dashboard") {
                microcar_boot_dashboard();
            } else {
                microcar_boot();
            }
        }

        sim_ffi::flush_trace();
        self.booted = true;
    }

    fn step(&mut self, _now: Tick, machine: &mut Machine) {
        if !self.booted {
            return;
        }

        let _guard = machine.activate();
        unsafe {
            sim_scheduler_tick();
        }
        sim_ffi::flush_trace();
    }
}

// ── Zephyr Dashboard Firmware ─────────────────────────────────────────────

/// Firmware adapter for the Zephyr dashboard ECU.
///
/// Only available when the `zephyr` feature is enabled.
/// Uses `sim_zephyr_scheduler_tick()` to advance the Zephyr scheduler
/// one cycle per step.
#[cfg(feature = "zephyr")]
pub struct ZephyrDashboardFirmware {
    booted: bool,
}

#[cfg(feature = "zephyr")]
impl ZephyrDashboardFirmware {
    pub fn new() -> Self {
        Self { booted: false }
    }
}

#[cfg(feature = "zephyr")]
impl Firmware for ZephyrDashboardFirmware {
    fn init(&mut self, machine: &mut Machine) {
        let _guard = machine.activate();
        unsafe {
            microcar_boot_dashboard_zephyr();
        }
        sim_ffi::flush_trace();
        self.booted = true;
    }

    fn step(&mut self, _now: Tick, machine: &mut Machine) {
        if !self.booted {
            return;
        }

        let _guard = machine.activate();
        unsafe {
            sim_zephyr_scheduler_tick();
        }
        sim_ffi::flush_trace();
    }
}
