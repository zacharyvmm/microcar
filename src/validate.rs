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

// ECU resolution is centralised in the crate root via
// `microcar::ECU_CATEGORY_PATTERNS` and `microcar::resolve_ecu_category`.
// This module imports them; it does NOT maintain its own keyword list.

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
/// canonical ECU category it will boot, or `None` if it resolves to the generic
/// `microcar_boot()` fallback.
///
/// Delegates to [`crate::resolve_ecu_category`], the single source of truth
/// shared with [`MicrocarFirmware::ecu_type`](crate::MicrocarFirmware).
pub fn resolve_ecu(firmware: Option<&str>, name: &str) -> Option<&'static str> {
    crate::resolve_ecu_category(firmware, name)
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
                        crate::ECU_CATEGORY_PATTERNS
                            .iter()
                            .map(|(_, cat)| *cat)
                            .collect::<BTreeSet<_>>()
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
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
            Some(kw @ ("powertrain" | "bms" | "dashboard")) if drivetrain.is_none() => {
                drivetrain = Some(format!("{kw} ('{}')", m.name));
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

    // 5. External-actor protocol frames (Stage C2).
    validate_external_actor_frames(scenario)?;

    Ok(())
}

// ── External-actor protocol IDs / node IDs (Stage C) ──────────────────────
const MSG_EVSE_EVENT: u32 = 0x610;
const MSG_OTA_REQUEST: u32 = 0x630;
const MSG_OTA_CHUNK: u32 = 0x631;
const MSG_OTA_FINISH: u32 = 0x632;
const NODE_EVSE: u8 = 6;
const NODE_OTA_TOOL: u8 = 7;

/// Validate `[[bus_inject]]` frames for the reserved external-actor CAN IDs
/// (`0x610` EVSE_EVENT and `0x630..0x632` OTA). Enforces the Stage C2 rules:
/// the sender is a firmware-less actor attached to the named bus; byte 0 is the
/// reserved node id; payload length and enum ranges match; request ids are
/// nonzero; OTA chunk indexes start at 0 and increase by 1 within
/// `total_chunks`; `OTA_FINISH.total_chunks` equals the request's declared and
/// observed chunk counts; no handshake precedes a plug and no chunk precedes an
/// `OTA_REQUEST`. Frames are evaluated in `at_ms` order.
fn validate_external_actor_frames(scenario: &Scenario) -> Result<(), ValidationError> {
    use std::collections::BTreeMap;

    // A sender "has firmware" if its name/firmware resolves to a real ECU.
    let is_ecu = |name: &str| -> bool {
        scenario
            .machine
            .iter()
            .find(|m| m.name == name)
            .map(|m| resolve_ecu(m.firmware.as_deref(), &m.name).is_some())
            .unwrap_or(false)
    };
    // bus name -> attached machine names.
    let mut bus_members: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for b in &scenario.bus {
        for n in &b.node {
            bus_members
                .entry(n.bus.as_str())
                .or_default()
                .insert(n.machine.as_str());
        }
    }

    // Frames of interest, in stable time order.
    let mut frames: Vec<&sim_world::scenario::BusInjectDef> = scenario
        .bus_inject
        .iter()
        .filter(|bi| {
            matches!(
                bi.id,
                MSG_EVSE_EVENT | MSG_OTA_REQUEST | MSG_OTA_CHUNK | MSG_OTA_FINISH
            )
        })
        .collect();
    frames.sort_by_key(|bi| bi.at_ms);

    let mut plugged = false;
    let mut ota_totals: BTreeMap<u8, u8> = BTreeMap::new();
    let mut ota_seen: BTreeMap<u8, u8> = BTreeMap::new();

    for bi in frames {
        let reserved = if bi.id == MSG_EVSE_EVENT {
            NODE_EVSE
        } else {
            NODE_OTA_TOOL
        };
        if is_ecu(&bi.sender) {
            return Err(ValidationError::new(
                "actor-has-firmware",
                format!(
                    "actor '{}' sends {:#05x} but resolves to a firmware ECU",
                    bi.sender, bi.id
                ),
            ));
        }
        if !bus_members
            .get(bi.bus.as_str())
            .map(|s| s.contains(bi.sender.as_str()))
            .unwrap_or(false)
        {
            return Err(ValidationError::new(
                "actor-not-on-bus",
                format!("actor '{}' is not attached to bus '{}'", bi.sender, bi.bus),
            ));
        }
        let want_len = match bi.id {
            MSG_EVSE_EVENT => 6,
            MSG_OTA_REQUEST => 4,
            MSG_OTA_CHUNK => 8,
            MSG_OTA_FINISH => 7,
            _ => unreachable!(),
        };
        if bi.data.len() != want_len {
            return Err(ValidationError::new(
                "bad-payload-length",
                format!(
                    "{:#05x} payload is {} bytes, expected {}",
                    bi.id,
                    bi.data.len(),
                    want_len
                ),
            ));
        }
        if bi.data[0] != reserved {
            return Err(ValidationError::new(
                "bad-source-node",
                format!(
                    "{:#05x} payload byte 0 is {}, expected reserved node {}",
                    bi.id, bi.data[0], reserved
                ),
            ));
        }

        match bi.id {
            MSG_EVSE_EVENT => {
                let event = bi.data[1];
                if event > 3 {
                    return Err(ValidationError::new(
                        "bad-enum-value",
                        format!("EVSE_EVENT event {event} out of range 0..3"),
                    ));
                }
                if bi.data[2] == 0 {
                    return Err(ValidationError::new(
                        "zero-request-id",
                        "EVSE_EVENT request_id must be nonzero".to_string(),
                    ));
                }
                if event == 1 {
                    plugged = true;
                } else if event == 2 && !plugged {
                    return Err(ValidationError::new(
                        "handshake-before-plug",
                        "HANDSHAKE_OK before any PLUG event".to_string(),
                    ));
                }
            }
            MSG_OTA_REQUEST => {
                let request_id = bi.data[1];
                if request_id == 0 {
                    return Err(ValidationError::new(
                        "zero-request-id",
                        "OTA_REQUEST request_id must be nonzero".to_string(),
                    ));
                }
                ota_totals.insert(request_id, bi.data[3]);
                ota_seen.entry(request_id).or_insert(0);
            }
            MSG_OTA_CHUNK => {
                let request_id = bi.data[1];
                if request_id == 0 {
                    return Err(ValidationError::new(
                        "zero-request-id",
                        "OTA_CHUNK request_id must be nonzero".to_string(),
                    ));
                }
                let chunk_index = bi.data[2];
                let Some(&total) = ota_totals.get(&request_id) else {
                    return Err(ValidationError::new(
                        "chunk-before-request",
                        format!("OTA_CHUNK for request {request_id} before its OTA_REQUEST"),
                    ));
                };
                let seen = ota_seen.entry(request_id).or_insert(0);
                if chunk_index != *seen {
                    return Err(ValidationError::new(
                        "ota-chunk-order",
                        format!(
                            "OTA_CHUNK index {chunk_index} for request {request_id}, expected {seen}"
                        ),
                    ));
                }
                if chunk_index >= total {
                    return Err(ValidationError::new(
                        "ota-chunk-order",
                        format!("OTA_CHUNK index {chunk_index} >= total_chunks {total}"),
                    ));
                }
                *seen += 1;
            }
            MSG_OTA_FINISH => {
                let request_id = bi.data[1];
                if request_id == 0 {
                    return Err(ValidationError::new(
                        "zero-request-id",
                        "OTA_FINISH request_id must be nonzero".to_string(),
                    ));
                }
                let finish_total = bi.data[2];
                let declared = ota_totals.get(&request_id).copied();
                let observed = ota_seen.get(&request_id).copied().unwrap_or(0);
                if declared != Some(finish_total) || observed != finish_total {
                    return Err(ValidationError::new(
                        "ota-finish-mismatch",
                        format!(
                            "OTA_FINISH total {finish_total} != declared {:?} / observed {observed}",
                            declared
                        ),
                    ));
                }
            }
            _ => {}
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

    // ── Stage C2: external-actor protocol validation ─────────────────────

    fn actors(injects: &str) -> String {
        format!(
            r#"
name = "actors"
duration_ms = 100
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
[[machine]]
id = 2
name = "evse"
[[machine]]
id = 3
name = "ota_tool"
[[bus]]
name = "vcan0"
type = "can"
latency_us = 500
[[bus.node]]
bus = "vcan0"
machine = "gateway"
[[bus.node]]
bus = "vcan0"
machine = "evse"
[[bus.node]]
bus = "vcan0"
machine = "ota_tool"
{injects}
"#
        )
    }

    fn err_kind(s: &str) -> &'static str {
        validate_scenario(&scenario(s)).unwrap_err().kind
    }

    fn inject(at: u64, sender: &str, id: u32, data: &str) -> String {
        format!(
            "[[bus_inject]]\nat_ms = {at}\nbus = \"vcan0\"\nsender = \"{sender}\"\nid = {id}\ndata = {data}\n"
        )
    }

    #[test]
    fn valid_evse_flow_passes() {
        let s = actors(&format!(
            "{}{}",
            inject(10, "evse", 0x610, "[6, 1, 1, 64, 80, 0]"),
            inject(20, "evse", 0x610, "[6, 2, 2, 64, 80, 0]"),
        ));
        assert!(validate_scenario(&scenario(&s)).is_ok());
    }

    #[test]
    fn handshake_before_plug_rejected() {
        let s = actors(&inject(10, "evse", 0x610, "[6, 2, 1, 64, 80, 0]"));
        assert_eq!(err_kind(&s), "handshake-before-plug");
    }

    #[test]
    fn actor_frame_from_ecu_rejected() {
        let s = actors(&inject(10, "gateway", 0x610, "[6, 1, 1, 64, 80, 0]"));
        assert_eq!(err_kind(&s), "actor-has-firmware");
    }

    #[test]
    fn bad_source_node_rejected() {
        let s = actors(&inject(10, "evse", 0x610, "[5, 1, 1, 64, 80, 0]"));
        assert_eq!(err_kind(&s), "bad-source-node");
    }

    #[test]
    fn bad_payload_length_rejected() {
        let s = actors(&inject(10, "evse", 0x610, "[6, 1, 1, 64, 80]"));
        assert_eq!(err_kind(&s), "bad-payload-length");
    }

    #[test]
    fn zero_request_id_rejected() {
        let s = actors(&inject(10, "evse", 0x610, "[6, 1, 0, 64, 80, 0]"));
        assert_eq!(err_kind(&s), "zero-request-id");
    }

    #[test]
    fn valid_ota_sequence_passes() {
        let s = actors(&format!(
            "{}{}{}{}",
            inject(10, "ota_tool", 0x630, "[7, 1, 0, 2]"),
            inject(20, "ota_tool", 0x631, "[7, 1, 0, 1, 2, 3, 4, 5]"),
            inject(30, "ota_tool", 0x631, "[7, 1, 1, 6, 7, 8, 9, 10]"),
            inject(40, "ota_tool", 0x632, "[7, 1, 2, 0, 0, 0, 0]"),
        ));
        assert!(validate_scenario(&scenario(&s)).is_ok());
    }

    #[test]
    fn ota_chunk_before_request_rejected() {
        let s = actors(&inject(10, "ota_tool", 0x631, "[7, 1, 0, 1, 2, 3, 4, 5]"));
        assert_eq!(err_kind(&s), "chunk-before-request");
    }

    #[test]
    fn ota_chunk_out_of_order_rejected() {
        let s = actors(&format!(
            "{}{}",
            inject(10, "ota_tool", 0x630, "[7, 1, 0, 2]"),
            inject(20, "ota_tool", 0x631, "[7, 1, 1, 6, 7, 8, 9, 10]"),
        ));
        assert_eq!(err_kind(&s), "ota-chunk-order");
    }

    #[test]
    fn ota_finish_mismatch_rejected() {
        let s = actors(&format!(
            "{}{}{}",
            inject(10, "ota_tool", 0x630, "[7, 1, 0, 2]"),
            inject(20, "ota_tool", 0x631, "[7, 1, 0, 1, 2, 3, 4, 5]"),
            inject(30, "ota_tool", 0x632, "[7, 1, 2, 0, 0, 0, 0]"),
        ));
        assert_eq!(err_kind(&s), "ota-finish-mismatch");
    }
}
