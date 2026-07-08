//! Microcar-specific scenario validation.
//!
//! costar's [`Scenario::from_file`](sim_world::scenario::Scenario::from_file)
//! already rejects *structural* problems (duplicate machine IDs, unknown bus /
//! link / fault references, malformed TOML, out-of-range integers, …) with a
//! [`ScenarioError`](sim_world::scenario::ScenarioError). Those are generic
//! simulation-infrastructure checks and belong in costar.
//!
//! This module adds the *automotive-semantic* checks that only make sense for
//! the microcar vehicle model — per the dogfood plan, "automotive-specific
//! behavior belongs in `microcar`". It is the validation backbone of the
//! `toml_zoo` dogfood lane: every malformed scenario must produce a *structured*
//! error and the binary must never panic.
//!
//! The checks (fail-fast, first error wins):
//!
//! 1. `unknown-firmware` — a machine names a `firmware` that does not resolve to
//!    any recognised ECU (it would otherwise silently fall back to the generic
//!    boot, masking a typo'd firmware path).
//! 2. `missing-gateway` — a drivetrain ECU (powertrain/bms/dashboard) is present
//!    but there is no gateway ECU. The gateway is the vehicle-mode authority and
//!    bus bridge; a drivetrain without one is not a valid vehicle.
//! 3. `duplicate-bus-node` — the same machine is attached to the same bus twice.
//! 4. `drive-without-powertrain` — a `driver_input` requests throttle but no
//!    powertrain ECU exists to command torque.

use std::collections::BTreeSet;

use sim_world::scenario::Scenario;

/// Canonical ECU keywords, in the same precedence order as
/// [`MicrocarFirmware::ecu_type`](crate::MicrocarFirmware) uses internally.
/// A firmware path or machine name that contains / starts with one of these
/// resolves to that ECU; anything else falls back to the generic boot.
pub const KNOWN_ECU_KEYWORDS: &[&str] = &[
    "priority_inversion",
    "lifecycle_stress",
    "net_demo",
    "storage_demo",
    "bt_demo",
    "diagnostics",
    "gateway",
    "powertrain",
    "bms",
    "dashboard",
];

/// A structured validation failure: a stable machine-readable `kind` tag plus a
/// human-readable `message`. The `kind` is what the `toml_zoo` lane asserts on,
/// so it must stay stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Stable, machine-readable error category (e.g. `"missing-gateway"`).
    pub kind: &'static str,
    /// Human-readable detail.
    pub message: String,
}

impl ValidationError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Resolve a machine (its optional `firmware` path and its `name`) to the
/// canonical ECU keyword it will boot, or `None` if it resolves to the generic
/// `microcar_boot()` fallback.
///
/// This mirrors `MicrocarFirmware::ecu_type`: firmware-path substring match
/// first (in [`KNOWN_ECU_KEYWORDS`] order), then a machine-name prefix match.
pub fn resolve_ecu(firmware: Option<&str>, name: &str) -> Option<&'static str> {
    if let Some(fw) = firmware {
        for kw in KNOWN_ECU_KEYWORDS {
            if fw.contains(kw) {
                return Some(kw);
            }
        }
    }
    for kw in KNOWN_ECU_KEYWORDS {
        if name.starts_with(kw) {
            return Some(kw);
        }
    }
    None
}

/// Run the microcar-semantic validation over a (already costar-validated)
/// scenario. Returns the first [`ValidationError`], or `Ok(())` if the scenario
/// is a well-formed vehicle.
pub fn validate_scenario(scenario: &Scenario) -> Result<(), ValidationError> {
    // 1. Unknown ECU firmware.
    for m in &scenario.machine {
        if let Some(fw) = m.firmware.as_deref() {
            if resolve_ecu(Some(fw), &m.name).is_none() {
                return Err(ValidationError::new(
                    "unknown-firmware",
                    format!(
                        "machine '{}' firmware '{}' does not name a known ECU (expected one of: {})",
                        m.name,
                        fw,
                        KNOWN_ECU_KEYWORDS.join(", ")
                    ),
                ));
            }
        }
    }

    // 2. Missing gateway (only when a drivetrain ECU is present).
    let mut has_gateway = false;
    let mut drivetrain: Option<String> = None;
    for m in &scenario.machine {
        match resolve_ecu(m.firmware.as_deref(), &m.name) {
            Some("gateway") => has_gateway = true,
            Some(kw @ ("powertrain" | "bms" | "dashboard")) => {
                if drivetrain.is_none() {
                    drivetrain = Some(format!("{kw} ('{}')", m.name));
                }
            }
            _ => {}
        }
    }
    if !has_gateway {
        if let Some(dt) = drivetrain {
            return Err(ValidationError::new(
                "missing-gateway",
                format!(
                    "scenario has a {dt} ECU but no gateway ECU \
                     (the gateway is the vehicle-mode authority and bus bridge)"
                ),
            ));
        }
    }

    // 3. Duplicate bus node.
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for b in &scenario.bus {
        for n in &b.node {
            if !seen.insert((n.bus.as_str(), n.machine.as_str())) {
                return Err(ValidationError::new(
                    "duplicate-bus-node",
                    format!(
                        "machine '{}' is attached to bus '{}' more than once",
                        n.machine, n.bus
                    ),
                ));
            }
        }
    }

    // 4. Drive without powertrain.
    let has_powertrain = scenario
        .machine
        .iter()
        .any(|m| resolve_ecu(m.firmware.as_deref(), &m.name) == Some("powertrain"));
    if !has_powertrain {
        for inp in &scenario.input {
            if inp.input_type == "driver_input" && inp.throttle_percent.unwrap_or(0) > 0 {
                return Err(ValidationError::new(
                    "drive-without-powertrain",
                    format!(
                        "driver_input at {} ms requests throttle {}% but the scenario \
                         has no powertrain ECU to command torque",
                        inp.at_ms,
                        inp.throttle_percent.unwrap_or(0)
                    ),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(toml: &str) -> Scenario {
        Scenario::from_str(toml).expect("costar validation should pass for these test inputs")
    }

    // A minimal well-formed vehicle: gateway + powertrain + bms + dashboard.
    const HEALTHY: &str = r#"
name = "healthy"
duration_ms = 100
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
[[machine]]
id = 2
name = "powertrain"
firmware = "firmware/powertrain_ecu"
[[machine]]
id = 3
name = "bms"
firmware = "firmware/bms_ecu"
[[machine]]
id = 4
name = "dashboard"
firmware = "firmware/dashboard_ecu"
[[bus]]
name = "vcan0"
type = "can"
latency_us = 500
[[bus.node]]
bus = "vcan0"
machine = "gateway"
[[bus.node]]
bus = "vcan0"
machine = "powertrain"
"#;

    #[test]
    fn healthy_scenario_passes() {
        assert!(validate_scenario(&scenario(HEALTHY)).is_ok());
    }

    #[test]
    fn resolve_ecu_matches_known_firmware() {
        assert_eq!(
            resolve_ecu(Some("firmware/gateway_ecu"), "m"),
            Some("gateway")
        );
        assert_eq!(resolve_ecu(Some("firmware/bms_ecu"), "m"), Some("bms"));
        assert_eq!(
            resolve_ecu(Some("firmware/dashboard_ecu"), "m"),
            Some("dashboard")
        );
        assert_eq!(
            resolve_ecu(Some("firmware/diagnostics_tool_ecu"), "m"),
            Some("diagnostics")
        );
        // Name-prefix fallback (firmware path has no keyword).
        assert_eq!(
            resolve_ecu(Some("firmware/custom"), "gateway_telemetry"),
            Some("gateway")
        );
        // Unknown.
        assert_eq!(resolve_ecu(Some("firmware/mystery_ecu"), "node_x"), None);
        // No firmware, no matching name.
        assert_eq!(resolve_ecu(None, "node_a"), None);
    }

    #[test]
    fn unknown_firmware_is_rejected() {
        let toml = r#"
name = "bad-fw"
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
[[machine]]
id = 2
name = "widget"
firmware = "firmware/widget_ecu"
"#;
        let e = validate_scenario(&scenario(toml)).unwrap_err();
        assert_eq!(e.kind, "unknown-firmware");
    }

    #[test]
    fn missing_gateway_is_rejected() {
        let toml = r#"
name = "no-gw"
[[machine]]
id = 1
name = "powertrain"
firmware = "firmware/powertrain_ecu"
[[machine]]
id = 2
name = "bms"
firmware = "firmware/bms_ecu"
"#;
        let e = validate_scenario(&scenario(toml)).unwrap_err();
        assert_eq!(e.kind, "missing-gateway");
    }

    #[test]
    fn subsystem_demo_without_gateway_is_allowed() {
        // A single storage-demo ECU (no drivetrain) needs no gateway.
        let toml = r#"
name = "storage-demo"
[[machine]]
id = 1
name = "storage"
firmware = "firmware/storage_demo_ecu"
"#;
        assert!(validate_scenario(&scenario(toml)).is_ok());
    }

    #[test]
    fn duplicate_bus_node_is_rejected() {
        let toml = r#"
name = "dup-node"
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
[[machine]]
id = 2
name = "powertrain"
firmware = "firmware/powertrain_ecu"
[[bus]]
name = "vcan0"
type = "can"
latency_us = 500
[[bus.node]]
bus = "vcan0"
machine = "gateway"
[[bus.node]]
bus = "vcan0"
machine = "gateway"
"#;
        let e = validate_scenario(&scenario(toml)).unwrap_err();
        assert_eq!(e.kind, "duplicate-bus-node");
    }

    #[test]
    fn drive_without_powertrain_is_rejected() {
        let toml = r#"
name = "drive-no-pt"
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
[[machine]]
id = 2
name = "dashboard"
firmware = "firmware/dashboard_ecu"
[[input]]
at_ms = 500
type = "driver_input"
throttle_percent = 40
brake_pressed = false
"#;
        let e = validate_scenario(&scenario(toml)).unwrap_err();
        assert_eq!(e.kind, "drive-without-powertrain");
    }

    #[test]
    fn zero_throttle_without_powertrain_is_allowed() {
        let toml = r#"
name = "coast-no-pt"
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
[[machine]]
id = 2
name = "dashboard"
firmware = "firmware/dashboard_ecu"
[[input]]
at_ms = 500
type = "driver_input"
throttle_percent = 0
brake_pressed = true
"#;
        assert!(validate_scenario(&scenario(toml)).is_ok());
    }
}
