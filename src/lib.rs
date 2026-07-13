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

use sim_core::Tick;
use sim_world::firmware::Firmware;
use sim_world::Machine;


pub mod validate;
// C ABI functions from the compiled firmware / sim-ffi.
extern "C" {
    fn microcar_boot();
    fn microcar_boot_gateway();
    fn microcar_boot_powertrain();
    fn microcar_boot_bms();
    fn microcar_boot_dashboard();
    fn microcar_boot_priority_inversion();
    fn microcar_boot_lifecycle_stress();
    fn microcar_boot_net_demo();
    fn microcar_boot_storage_demo();
    fn microcar_boot_bt_demo();
    fn microcar_boot_ota_tool();
    fn microcar_boot_telematics();
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

// ── Shared ECU resolution (single source of truth) ────────────────────────
//
// Used by both `MicrocarFirmware::ecu_type()` (for boot dispatch) and
// `validate::resolve_ecu()` (for automotive-semantic validation) so the two
// never drift.  Patterns are ordered most-specific-first; `contains()`
// returns the first match.
//
// Each entry is (firmware_path_substring, canonical_ecu_category).
// The category is the broad ECU kind used by validation and boot dispatch.

/// Shared ECU resolution patterns: (path substring, canonical category).
/// Ordered most-specific-first — e.g. `gateway_diag_clearbug` before
/// `gateway_diag` before `gateway`.
pub const ECU_CATEGORY_PATTERNS: &[(&str, &str)] = &[
    // Demo / test ECUs
    ("priority_inversion", "priority_inversion"),
    ("lifecycle_stress", "lifecycle_stress"),
    ("net_demo", "net_demo"),
    ("storage_demo", "storage_demo"),
    ("bt_demo", "bt_demo"),
    ("ota_tool", "ota_tool"),
    ("telematics", "telematics"),
    // Diagnostics variants (specific → broad)
    ("gateway_diag_clearbug", "diagnostics"),
    ("gateway_diag_clear", "diagnostics"),
    ("gateway_diag_startdrivebug", "diagnostics"),
    ("gateway_diag_startdrive", "diagnostics"),
    ("gateway_diag_fault", "diagnostics"),
    ("gateway_diag", "diagnostics"),
    ("powertrain_diag_service_bug", "powertrain"),
    ("powertrain_diag_service", "powertrain"),
    // OTA variants
    ("gateway_ota_badcrc", "gateway"),
    ("gateway_ota_intwrite", "gateway"),
    ("gateway_ota_badhealth", "gateway"),
    ("gateway_ota_powercut", "gateway"),
    ("gateway_ota_crcbug", "gateway"),
    ("gateway_ota", "gateway"),
    // Charging variants
    ("gateway_charging", "gateway"),
    ("powertrain_charging", "powertrain"),
    // Broad base ECUs (checked LAST)
    ("diagnostics", "diagnostics"),
    ("gateway", "gateway"),
    ("powertrain", "powertrain"),
    ("bms", "bms"),
    ("dashboard", "dashboard"),
];

/// Resolve a machine's firmware path to its canonical ECU category.
///
/// Matches the firmware path against [`ECU_CATEGORY_PATTERNS`] using
/// substring `contains()`, most-specific-first.  Falls back to matching
/// against the machine name prefix if firmware path yields no match.
/// Returns `None` for unknown firmware.
pub fn resolve_ecu_category(firmware: Option<&str>, name: &str) -> Option<&'static str> {
    if let Some(path) = firmware {
        for (pattern, category) in ECU_CATEGORY_PATTERNS {
            if path.contains(pattern) {
                return Some(category);
            }
        }
        // Name-based fallback: only when firmware IS provided but its path
        // didn't match any pattern.  Machines without firmware (external
        // actors like evse/ota_tool) must not resolve to a known ECU.
        for (pattern, category) in ECU_CATEGORY_PATTERNS {
            if name.starts_with(pattern) {
                return Some(category);
            }
        }
    }
    None
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

    /// Resolve this firmware instance to its canonical ECU category.
    ///
    /// Uses the shared [`ECU_CATEGORY_PATTERNS`] table so that both boot
    /// dispatch and automotive-semantic validation resolve firmware the
    /// same way — preventing the two from drifting.
    fn ecu_type(&self) -> &str {
        if let Some(category) = resolve_ecu_category(
            self.firmware_path.as_deref(),
            &self.name,
        ) {
            category
        } else {
            // Fallback for unknown firmware: use the machine name, which
            // in practice is one of "gateway", "powertrain", "bms", …
            &self.name
        }
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
            } else if ecu.starts_with("ota_tool") {
                microcar_boot_ota_tool();
            } else if ecu.starts_with("telematics") {
                microcar_boot_telematics();
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
