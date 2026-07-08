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

pub mod validate;

use sim_core::Tick;
use sim_world::firmware::Firmware;
use sim_world::Machine;

// C ABI functions from the compiled firmware / sim-ffi.
extern "C" {
    fn microcar_boot();
    fn microcar_boot_gateway();
    fn microcar_boot_gateway_diag();
    fn microcar_boot_gateway_diag_fault();
    fn microcar_boot_powertrain();
    fn microcar_boot_powertrain_diag_service();
    fn microcar_boot_powertrain_diag_service_bug();
    fn microcar_boot_gateway_charging();
    fn microcar_boot_gateway_ota();
    fn microcar_boot_gateway_ota_badcrc();
    fn microcar_boot_gateway_ota_intwrite();
    fn microcar_boot_gateway_ota_badhealth();
    fn microcar_boot_gateway_ota_powercut();
    fn microcar_boot_gateway_ota_crcbug();
    fn microcar_boot_powertrain_charging();
    fn microcar_boot_bms();
    fn microcar_boot_dashboard();
    fn microcar_boot_diagnostics_tool();
    fn microcar_boot_priority_inversion();
    fn microcar_boot_lifecycle_stress();
    fn microcar_boot_net_demo();
    fn microcar_boot_storage_demo();
    fn microcar_boot_bt_demo();
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
            if path.contains("net_demo") {
                return "net_demo";
            }
            if path.contains("storage_demo") {
                return "storage_demo";
            }
            if path.contains("bt_demo") {
                return "bt_demo";
            }
            if path.contains("gateway_diag_fault") {
                return "gateway_diag_fault";
            }
            if path.contains("gateway_diag") {
                return "gateway_diag";
            }
            if path.contains("powertrain_diag_service_bug") {
                return "powertrain_diag_service_bug";
            }
            if path.contains("powertrain_diag_service") {
                return "powertrain_diag_service";
            }
            if path.contains("gateway_ota_badcrc") {
                return "gateway_ota_badcrc";
            }
            if path.contains("gateway_ota_intwrite") {
                return "gateway_ota_intwrite";
            }
            if path.contains("gateway_ota_badhealth") {
                return "gateway_ota_badhealth";
            }
            if path.contains("gateway_ota_powercut") {
                return "gateway_ota_powercut";
            }
            if path.contains("gateway_ota_crcbug") {
                return "gateway_ota_crcbug";
            }
            if path.contains("gateway_ota") {
                return "gateway_ota";
            }
            if path.contains("gateway_charging") {
                return "gateway_charging";
            }
            if path.contains("powertrain_charging") {
                return "powertrain_charging";
            }
            if path.contains("diagnostics") {
                return "diagnostics";
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
            } else if ecu.starts_with("net_demo") {
                microcar_boot_net_demo();
            } else if ecu.starts_with("storage_demo") {
                microcar_boot_storage_demo();
            } else if ecu.starts_with("bt_demo") {
                microcar_boot_bt_demo();
            } else if ecu.starts_with("gateway_diag_fault") {
                microcar_boot_gateway_diag_fault();
            } else if ecu.starts_with("gateway_diag") {
                microcar_boot_gateway_diag();
            } else if ecu.starts_with("powertrain_diag_service_bug") {
                microcar_boot_powertrain_diag_service_bug();
            } else if ecu.starts_with("powertrain_diag_service") {
                microcar_boot_powertrain_diag_service();
            } else if ecu.starts_with("gateway_ota_badcrc") {
                microcar_boot_gateway_ota_badcrc();
            } else if ecu.starts_with("gateway_ota_intwrite") {
                microcar_boot_gateway_ota_intwrite();
            } else if ecu.starts_with("gateway_ota_badhealth") {
                microcar_boot_gateway_ota_badhealth();
            } else if ecu.starts_with("gateway_ota_powercut") {
                microcar_boot_gateway_ota_powercut();
            } else if ecu.starts_with("gateway_ota_crcbug") {
                microcar_boot_gateway_ota_crcbug();
            } else if ecu.starts_with("gateway_ota") {
                microcar_boot_gateway_ota();
            } else if ecu.starts_with("gateway_charging") {
                microcar_boot_gateway_charging();
            } else if ecu.starts_with("powertrain_charging") {
                microcar_boot_powertrain_charging();
            } else if ecu.starts_with("diagnostics") {
                microcar_boot_diagnostics_tool();
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
