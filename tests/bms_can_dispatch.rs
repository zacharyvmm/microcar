//! Task 11 regression: BMS CAN dispatch must route only `MC_MSG_PLANT_SENSORS`
//! (0x500) through the plant-sensor decoder (`handle_plant_sensors`).
//! `MC_MSG_BMS_STATUS` (0x200) is the BMS's own output message and uses a
//! completely different wire layout; it must never be fed back through the
//! plant decoder.
//!
//! All cases share one long-lived World — repeatedly tearing down FreeRTOS
//! BMS firmware Worlds in-process is a known simulator limitation.

use std::sync::Arc;

use microcar::MicrocarFirmware;
use sim_world::firmware::FirmwareFactory;
use sim_world::scenario::Scenario;
use sim_world::{BoardConfig, World};

const SCENARIO: &str = r#"
name = "bms_can_dispatch_test"
duration_ms = 800

[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
rtos = "freertos"

[[machine]]
id = 3
name = "bms"
firmware = "firmware/bms_ecu"
rtos = "freertos"

[[bus]]
name = "vcan0"
type = "can"
latency_us = 500

[[bus.node]]
bus = "vcan0"
machine = "gateway"

[[bus.node]]
bus = "vcan0"
machine = "bms"
"#;

const GATEWAY_BOARD: &str = r#"
[peripherals]
can0 = { device = "can", id = 0 }
flash0 = { device = "flash", id = 0 }
"#;

const BMS_BOARD: &str = r#"
[peripherals]
can0 = { device = "can", id = 0 }
"#;

const MC_NODE_PLANT: u64 = 100;
const BMS_MACHINE_ID: u64 = 3;
const CAN_ID_PLANT_SENSORS: u32 = 0x500;
const CAN_ID_BMS_STATUS: u32 = 0x200;
const CAN_ID_BMS_LIMITS: u32 = 0x201;
const CAN_ID_BMS_FAULT: u32 = 0x202;

fn build_world() -> World {
    let scenario = Scenario::from_str(SCENARIO).expect("parse scenario");
    let mut world = scenario.build_world().expect("build world");
    world.enable_owned_device_banks();
    world.enable_trace_v2();
    world
        .configure_machine_board(1, BoardConfig::from_str(GATEWAY_BOARD).expect("gw board"))
        .expect("configure gateway");
    world
        .configure_machine_board(3, BoardConfig::from_str(BMS_BOARD).expect("bms board"))
        .expect("configure bms");

    for m in &scenario.machine {
        if m.firmware.is_none() {
            continue;
        }
        let Some(machine) = world.machine_mut(m.id) else {
            continue;
        };
        let name = m.name.clone();
        let fw_path = m.firmware.clone().unwrap_or_default();
        let factory: FirmwareFactory = Arc::new(move || {
            Box::new(MicrocarFirmware::with_firmware_path(
                name.clone(),
                fw_path.clone(),
            ))
        });
        machine.load_firmware_from_factory(factory);
    }
    world
}

fn plant_sensor_payload(soc: u8, voltage_mv: u16, temp_c_x10: i16, current_ma: i16) -> Vec<u8> {
    vec![
        soc,
        (voltage_mv >> 8) as u8,
        voltage_mv as u8,
        ((temp_c_x10 as u16) >> 8) as u8,
        temp_c_x10 as u8,
        ((current_ma as u16) >> 8) as u8,
        current_ma as u8,
    ]
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(
        !hex.contains('\u{2026}'),
        "payload unexpectedly truncated: {hex}"
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex byte"))
        .collect()
}

fn decode_bms_status(bytes: &[u8]) -> (u16, i16, i16, u8, u8) {
    assert_eq!(bytes.len(), 8, "0x200 payload must be 8 bytes, got {bytes:?}");
    (
        u16::from_le_bytes([bytes[0], bytes[1]]),
        i16::from_le_bytes([bytes[2], bytes[3]]),
        i16::from_le_bytes([bytes[4], bytes[5]]),
        bytes[6],
        bytes[7],
    )
}

fn decode_bms_limits(bytes: &[u8]) -> (u8, u8) {
    assert_eq!(bytes.len(), 2, "0x201 payload must be 2 bytes, got {bytes:?}");
    (bytes[0], bytes[1])
}

fn bms_tx(recs: &[sim_core::TraceV2], id: u32) -> Vec<&sim_core::TraceV2> {
    recs.iter()
        .filter(|r| r.direction == "tx" && r.source == BMS_MACHINE_ID && r.message_id == id)
        .collect()
}

/// Single-World acceptance covering valid 0x500, short 0x500 rejection,
/// foreign 0x200 ignore, and correct 0x200/0x201 encoding across state bands.
#[test]
fn bms_plant_dispatch_acceptance() {
    let mut world = build_world();
    let mut t_us: u64 = 10_000;

    // ── 1. Foreign 0x200 must not be treated as plant input ─────────────
    let bogus_status: Vec<u8> = vec![0x10, 0xB4, 0x00, 0xFF, 0xFF, 0x00, 50, 7];
    world.inject_can_frame("vcan0", MC_NODE_PLANT, CAN_ID_BMS_STATUS, &bogus_status, t_us);
    t_us += 40_000;
    world.run_until(t_us).expect("run after foreign 0x200");
    let recs = world.drain_trace_v2();
    assert!(
        bms_tx(&recs, CAN_ID_BMS_STATUS).is_empty(),
        "0x200 input must not be plant-decoded or republished"
    );
    assert!(
        bms_tx(&recs, CAN_ID_BMS_FAULT).is_empty(),
        "bogus 0x200 must not raise a fault"
    );

    // ── 2. Valid 0x500 updates live state → 0x200 ───────────────────────
    t_us += 10_000;
    let payload = plant_sensor_payload(55, 48_000, 300, -1200);
    world.inject_can_frame("vcan0", MC_NODE_PLANT, CAN_ID_PLANT_SENSORS, &payload, t_us);
    t_us += 40_000;
    world.run_until(t_us).expect("run after valid 0x500");
    let recs = world.drain_trace_v2();
    let status_frames = bms_tx(&recs, CAN_ID_BMS_STATUS);
    assert!(
        !status_frames.is_empty(),
        "BMS must publish 0x200 after a valid plant sensor frame"
    );
    let (voltage_mv, current_ma, temp_c_x10, soc, seq) =
        decode_bms_status(&decode_hex(&status_frames.last().unwrap().payload_summary));
    assert_eq!(voltage_mv, 48_000);
    assert_eq!(current_ma, -1200);
    assert_eq!(temp_c_x10, 300);
    assert_eq!(soc, 55);
    assert!(seq >= 1);

    // ── 3. Short 0x500 rejected — does not corrupt state ────────────────
    t_us += 10_000;
    let short: Vec<u8> = vec![99, 0xFF, 0xFF, 0x23, 0x28, 0x00];
    world.inject_can_frame("vcan0", MC_NODE_PLANT, CAN_ID_PLANT_SENSORS, &short, t_us);
    t_us += 40_000;
    world.run_until(t_us).expect("run after short 0x500");
    let recs = world.drain_trace_v2();
    assert!(
        bms_tx(&recs, CAN_ID_BMS_STATUS).is_empty(),
        "rejected short frame must not trigger a status publish"
    );
    let limits = bms_tx(&recs, CAN_ID_BMS_LIMITS);
    assert!(!limits.is_empty(), "BMS must keep publishing periodic 0x201");
    let (max_torque, reason) =
        decode_bms_limits(&decode_hex(&limits.last().unwrap().payload_summary));
    assert_eq!(max_torque, 255, "short frame must not change torque limits");
    assert_eq!(reason, 0);

    // ── 4. Temperature bands → correct 0x200 / 0x201 ────────────────────
    let cases: [(i16, u8, u8); 4] = [
        (300, 0, 255),
        (650, 1, 255),
        (800, 2, 25),
        (950, 3, 0),
    ];
    for (temp, expect_reason, expect_torque) in cases {
        t_us += 10_000;
        let p = plant_sensor_payload(50, 40_000, temp, -800);
        world.inject_can_frame("vcan0", MC_NODE_PLANT, CAN_ID_PLANT_SENSORS, &p, t_us);
        t_us += 50_000;
        world.run_until(t_us).expect("run band");
        let recs = world.drain_trace_v2();

        let status = bms_tx(&recs, CAN_ID_BMS_STATUS);
        assert!(!status.is_empty(), "temp={temp}: expected 0x200");
        let (_, _, got_temp, _, _) =
            decode_bms_status(&decode_hex(&status.last().unwrap().payload_summary));
        assert_eq!(got_temp, temp, "temp={temp}: 0x200 temp mismatch");

        let limits = bms_tx(&recs, CAN_ID_BMS_LIMITS);
        assert!(!limits.is_empty(), "temp={temp}: expected 0x201");
        let (torque, reason) =
            decode_bms_limits(&decode_hex(&limits.last().unwrap().payload_summary));
        assert_eq!(reason, expect_reason, "temp={temp}: limits reason");
        assert_eq!(torque, expect_torque, "temp={temp}: max torque");
    }
}
