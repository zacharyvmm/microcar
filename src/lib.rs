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
    fn microcar_boot_gateway_diag();
    fn microcar_boot_gateway_diag_fault();
    fn microcar_boot_gateway_diag_clear();
    fn microcar_boot_gateway_diag_clearbug();
    fn microcar_boot_gateway_diag_startdrive();
    fn microcar_boot_gateway_diag_startdrivebug();
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
    // Gateway diagnosis variants (specific → broad; category is gateway, not diagnostics)
    ("gateway_diag_clearbug", "gateway"),
    ("gateway_diag_clear", "gateway"),
    ("gateway_diag_startdrivebug", "gateway"),
    ("gateway_diag_startdrive", "gateway"),
    ("gateway_diag_fault", "gateway"),
    ("gateway_diag", "gateway"),
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
    pub(crate) fn ecu_type(&self) -> &str {
        // Keep the detailed firmware variant for boot dispatch. Validation
        // intentionally resolves these variants to their broad ECU category,
        // but the firmware ABI exposes dedicated entry points for the
        // dogfood lanes and seeded-bug variants.
        if let Some(path) = self.firmware_path.as_deref() {
            const VARIANTS: &[&str] = &[
                "priority_inversion",
                "lifecycle_stress",
                "net_demo",
                "storage_demo",
                "bt_demo",
                "ota_tool",
                "telematics",
                "gateway_diag_clearbug",
                "gateway_diag_clear",
                "gateway_diag_startdrivebug",
                "gateway_diag_startdrive",
                "gateway_diag_fault",
                "gateway_diag",
                "powertrain_diag_service_bug",
                "powertrain_diag_service",
                "gateway_ota_badcrc",
                "gateway_ota_intwrite",
                "gateway_ota_badhealth",
                "gateway_ota_powercut",
                "gateway_ota_crcbug",
                "gateway_ota",
                "gateway_charging",
                "powertrain_charging",
                "diagnostics",
                "gateway",
                "powertrain",
                "bms",
                "dashboard",
            ];
            if let Some(variant) = VARIANTS.iter().find(|variant| path.contains(*variant)) {
                return variant;
            }
        }

        if let Some(category) = resolve_ecu_category(self.firmware_path.as_deref(), &self.name) {
            category
        } else {
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
            } else if ecu.starts_with("gateway_diag_clearbug") {
                microcar_boot_gateway_diag_clearbug();
            } else if ecu.starts_with("gateway_diag_clear") {
                microcar_boot_gateway_diag_clear();
            } else if ecu.starts_with("gateway_diag_startdrivebug") {
                microcar_boot_gateway_diag_startdrivebug();
            } else if ecu.starts_with("gateway_diag_startdrive") {
                microcar_boot_gateway_diag_startdrive();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each entry: (firmware_path, expected_ecu_category, expected_boot_variant)
    /// `ecu_type()` returns the FIRST matching variant from the `VARIANTS` list
    /// (which may be more specific than the category). `resolve_ecu_category()`
    /// returns the broad category used for validation.
    const CLASSIFICATION_TESTS: &[(&str, &str, &str)] = &[
        // Demo / test ECUs
        (
            "firmware/priority_inversion_demo",
            "priority_inversion",
            "priority_inversion",
        ),
        (
            "firmware/lifecycle_stress_ecu",
            "lifecycle_stress",
            "lifecycle_stress",
        ),
        ("firmware/net_demo_ecu", "net_demo", "net_demo"),
        ("firmware/storage_demo_ecu", "storage_demo", "storage_demo"),
        ("firmware/bt_demo_ecu", "bt_demo", "bt_demo"),
        // Tool ECUs
        ("firmware/ota_tool_ecu", "ota_tool", "ota_tool"),
        ("firmware/telematics_ecu", "telematics", "telematics"),
        (
            "firmware/diagnostics_tool_ecu",
            "diagnostics",
            "diagnostics",
        ),
        // Gateway diagnosis variants → category = gateway (not diagnostics!)
        (
            "firmware/gateway_diag_clearbug_ecu",
            "gateway",
            "gateway_diag_clearbug",
        ),
        (
            "firmware/gateway_diag_clear_ecu",
            "gateway",
            "gateway_diag_clear",
        ),
        (
            "firmware/gateway_diag_startdrivebug_ecu",
            "gateway",
            "gateway_diag_startdrivebug",
        ),
        (
            "firmware/gateway_diag_startdrive_ecu",
            "gateway",
            "gateway_diag_startdrive",
        ),
        (
            "firmware/gateway_diag_fault_ecu",
            "gateway",
            "gateway_diag_fault",
        ),
        ("firmware/gateway_diag_ecu", "gateway", "gateway_diag"),
        // Powertrain diagnosis variants
        (
            "firmware/powertrain_diag_service_bug_ecu",
            "powertrain",
            "powertrain_diag_service_bug",
        ),
        (
            "firmware/powertrain_diag_service_ecu",
            "powertrain",
            "powertrain_diag_service",
        ),
        // OTA variants
        (
            "firmware/gateway_ota_badcrc_ecu",
            "gateway",
            "gateway_ota_badcrc",
        ),
        (
            "firmware/gateway_ota_intwrite_ecu",
            "gateway",
            "gateway_ota_intwrite",
        ),
        (
            "firmware/gateway_ota_badhealth_ecu",
            "gateway",
            "gateway_ota_badhealth",
        ),
        (
            "firmware/gateway_ota_powercut_ecu",
            "gateway",
            "gateway_ota_powercut",
        ),
        (
            "firmware/gateway_ota_crcbug_ecu",
            "gateway",
            "gateway_ota_crcbug",
        ),
        ("firmware/gateway_ota_ecu", "gateway", "gateway_ota"),
        // Charging variants
        (
            "firmware/gateway_charging_ecu",
            "gateway",
            "gateway_charging",
        ),
        (
            "firmware/powertrain_charging_ecu",
            "powertrain",
            "powertrain_charging",
        ),
        // Base ECUs
        ("firmware/gateway_ecu", "gateway", "gateway"),
        ("firmware/powertrain_ecu", "powertrain", "powertrain"),
        ("firmware/bms_ecu", "bms", "bms"),
        ("firmware/dashboard_ecu", "dashboard", "dashboard"),
    ];

    #[test]
    fn ecu_category_table() {
        for (path, expected_category, _expected_boot) in CLASSIFICATION_TESTS {
            let category = resolve_ecu_category(Some(path), "test_machine");
            assert_eq!(
                category,
                Some(*expected_category),
                "path '{path}': expected category '{expected_category}', got {category:?}"
            );
        }
    }

    #[test]
    fn ecu_boot_variant_table() {
        for (path, _expected_category, expected_boot) in CLASSIFICATION_TESTS {
            let fw = MicrocarFirmware::with_firmware_path("test_machine", *path);
            let boot_variant = fw.ecu_type();
            assert_eq!(
                boot_variant, *expected_boot,
                "path '{path}': expected boot variant '{expected_boot}', got '{boot_variant}'"
            );
        }
    }
    #[test]
    fn gateway_diag_variants_are_category_gateway_not_diagnostics() {
        // This is the regression test for the R0 bug: gateway_diag* variants
        // must NOT resolve to "diagnostics" for validation purposes.
        let diag_paths = &[
            "firmware/gateway_diag_ecu",
            "firmware/gateway_diag_fault_ecu",
            "firmware/gateway_diag_clear_ecu",
            "firmware/gateway_diag_clearbug_ecu",
            "firmware/gateway_diag_startdrive_ecu",
            "firmware/gateway_diag_startdrivebug_ecu",
        ];
        for path in diag_paths {
            let category = resolve_ecu_category(Some(path), "test_machine");
            assert_eq!(
                category,
                Some("gateway"),
                "gateway_diag variant '{path}' must be category 'gateway', got {category:?}"
            );
        }
    }

    #[test]
    fn diagnostics_tool_is_still_diagnostics() {
        let category = resolve_ecu_category(Some("firmware/diagnostics_tool_ecu"), "test_machine");
        assert_eq!(category, Some("diagnostics"));
    }

    #[test]
    fn unknown_firmware_returns_none() {
        assert_eq!(
            resolve_ecu_category(Some("firmware/mystery_ecu"), "x"),
            None
        );
    }

    #[test]
    fn no_firmware_returns_none() {
        assert_eq!(resolve_ecu_category(None, "some_machine"), None);
    }
}
