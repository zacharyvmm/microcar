//! R4: gateway reboot downtime proves real firmware recovery.
//!
//! Extends the TOML fixture assertions with flash persistence, RTOS/config
//! identity, volatile CAN queue reset, heartbeat continuity/reset proof via
//! the CAN protocol (not just internal device inspection), and 100×
//! repeatability.

use std::sync::Arc;

use microcar::MicrocarFirmware;
use sim_devices::inspect::DeviceSnapshot;
use sim_world::firmware::FirmwareFactory;
use sim_world::scenario::Scenario;
use sim_world::{BoardConfig, RtosBackend, World};

const SCENARIO: &str = r#"
name = "b3_gateway_reboot_downtime"
duration_ms = 2000

[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
rtos = "freertos"

[[machine]]
id = 2
name = "powertrain"
firmware = "firmware/powertrain_ecu"
rtos = "freertos"

# Live bus observer: Trace v2 records CanTx only when a frame is *delivered*
# to a non-stopped receiver. During gateway downtime the gateway is removed /
# stopped, so without a third node powertrain heartbeats would be sent onto
# the bus but never appear in Trace v2 — making "sibling TX during downtime"
# unobservable. Dashboard stays up and completes those delivery edges.
[[machine]]
id = 4
name = "dashboard"
firmware = "firmware/dashboard_ecu"
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
machine = "powertrain"

[[bus.node]]
bus = "vcan0"
machine = "dashboard"

[[fault]]
at_ms = 1000
target = "machine.gateway"
type = "reboot"
downtime_ms = 150
"#;

const GATEWAY_BOARD: &str = r#"
[peripherals]
can0 = { device = "can", id = 0 }
flash0 = { device = "flash", id = 0 }
"#;

const CAN_BOARD: &str = r#"
[peripherals]
can0 = { device = "can", id = 0 }
"#;

const FLASH_MARKER: &[u8] = &[0x4D, 0x43, 0x52, 0x34, 0xB3, 0x01, 0x02, 0x03];

const CAN_ID_HEARTBEAT: u32 = 0x001;
const GATEWAY_MACHINE_ID: u64 = 1;
const POWERTRAIN_MACHINE_ID: u64 = 2;
const REBOOT_AT_US: u64 = 1_000_000;
const REBOOT_DOWNTIME_US: u64 = 150_000;

/// Decode a `mc_heartbeat_msg_t` (0x001) payload as written by
/// `send_heartbeat` in `firmware/{gateway,powertrain}_ecu/src/main.c`:
/// `data[0] = node_id`, `data[1..5] = uptime_ms` (big-endian, hand-packed).
fn decode_heartbeat(bytes: &[u8]) -> (u8, u32) {
    assert_eq!(
        bytes.len(),
        5,
        "0x001 payload must be 5 bytes, got {bytes:?}"
    );
    let node_id = bytes[0];
    let uptime_ms = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    (node_id, uptime_ms)
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(
        !hex.contains('\u{2026}'),
        "heartbeat payload unexpectedly truncated: {hex}"
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex byte"))
        .collect()
}

/// Every heartbeat (virtual_time, uptime_ms) transmitted by `source`.
fn heartbeats_from(recs: &[sim_core::TraceV2], source: u64) -> Vec<(u64, u32)> {
    recs.iter()
        .filter(|r| r.direction == "tx" && r.source == source && r.message_id == CAN_ID_HEARTBEAT)
        .map(|r| {
            let (node_id, uptime_ms) = decode_heartbeat(&decode_hex(&r.payload_summary));
            assert_eq!(
                node_id as u64, source,
                "heartbeat node_id must match Trace v2 source"
            );
            (r.virtual_time, uptime_ms)
        })
        .collect()
}

fn build_world() -> World {
    let scenario = Scenario::from_str(SCENARIO).expect("parse scenario");
    let mut world = scenario.build_world().expect("build world");
    world.enable_owned_device_banks();
    world.enable_trace_v2();

    world
        .configure_machine_board(1, BoardConfig::from_str(GATEWAY_BOARD).expect("gw board"))
        .expect("configure gateway board");
    for mid in [2u64, 4] {
        world
            .configure_machine_board(mid, BoardConfig::from_str(CAN_BOARD).expect("can board"))
            .unwrap_or_else(|_| panic!("configure machine {mid}"));
    }

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

    scenario.schedule_faults_to(&mut world);

    // Seed the global event queue so firmware runs before the reboot fault.
    world.inject_can_frame("vcan0", 99, 0xFFF, &[0], 1);

    world
}

fn write_flash_marker(world: &mut World) {
    world
        .with_machine_devices(1, || {
            let ok = sim_devices::with_flash_mut(0, |flash| {
                assert!(flash.erase_page(0), "erase page 0");
                assert!(flash.write_page(0, 0, FLASH_MARKER), "write flash marker");
            })
            .is_some();
            assert!(ok, "flash device 0 must exist on gateway");
        })
        .expect("gateway machine devices");
}

fn read_flash_marker(world: &mut World) -> Vec<u8> {
    world
        .with_machine_devices(1, || {
            sim_devices::with_flash(0, |flash| {
                (0..FLASH_MARKER.len())
                    .map(|i| flash.read(i).expect("flash byte in range"))
                    .collect::<Vec<_>>()
            })
            .expect("flash device 0")
        })
        .expect("gateway machine devices")
}

fn can_queue_lens(world: &mut World) -> (usize, usize) {
    world
        .with_machine_devices(1, || {
            let snap = DeviceSnapshot::collect_all();
            for s in snap {
                if let DeviceSnapshot::Can {
                    id: 0,
                    tx_queue_len,
                    rx_queue_len,
                    ..
                } = s
                {
                    return (tx_queue_len, rx_queue_len);
                }
            }
            panic!("CAN controller 0 missing after reboot");
        })
        .expect("gateway machine devices")
}

/// Heartbeat before/during/after reboot via decoded 0x001 Trace v2 payloads.
///
/// Powertrain continuity is proven by **uptime**, not only monotonic virtual
/// time: a sibling that itself rebooted could restart heartbeating from a
/// boot-range uptime while virtual time remains increasing.
fn assert_heartbeat_reboot_proof(recs: &[sim_core::TraceV2]) {
    let gw = heartbeats_from(recs, GATEWAY_MACHINE_ID);
    let mut pt = heartbeats_from(recs, POWERTRAIN_MACHINE_ID);
    pt.sort_by_key(|&(t, _)| t);

    const REBOOT_GRACE_US: u64 = 2_000;
    /// Firmware sends heartbeats every `MC_SAFETY_HEARTBEAT_INTERVAL_MS` (100ms).
    const HEARTBEAT_PERIOD_US: u64 = 100_000;
    /// Allow one missed beat plus tick/scheduling slack.
    const HEARTBEAT_GAP_TOLERANCE_US: u64 = HEARTBEAT_PERIOD_US + 50_000;
    /// Uptime values in this range are treated as "just booted".
    const BOOT_UPTIME_CEILING_MS: u32 = 200;

    let gw_before: Vec<_> = gw.iter().filter(|&&(t, _)| t < REBOOT_AT_US).collect();
    let gw_during: Vec<_> = gw
        .iter()
        .filter(|&&(t, _)| {
            (REBOOT_AT_US + REBOOT_GRACE_US..REBOOT_AT_US + REBOOT_DOWNTIME_US).contains(&t)
        })
        .collect();
    let gw_after: Vec<_> = gw
        .iter()
        .filter(|&&(t, _)| t >= REBOOT_AT_US + REBOOT_DOWNTIME_US)
        .collect();

    assert!(
        !gw_before.is_empty(),
        "gateway must heartbeat before reboot"
    );
    assert!(
        gw_during.is_empty(),
        "gateway must not heartbeat during its own downtime, got {gw_during:?}"
    );
    assert!(
        !gw_after.is_empty(),
        "gateway must heartbeat again after reboot"
    );

    let last_before_uptime = gw_before.last().unwrap().1;
    let first_after_uptime = gw_after.first().unwrap().1;
    if last_before_uptime > 0 {
        assert!(
            last_before_uptime >= 900,
            "gateway uptime should approach reboot, got {last_before_uptime}"
        );
        assert!(
            first_after_uptime < last_before_uptime,
            "gateway uptime must reset across reboot: before={last_before_uptime} after={first_after_uptime}"
        );
        assert!(
            first_after_uptime <= BOOT_UPTIME_CEILING_MS,
            "post-reboot gateway uptime must be in boot range, got {first_after_uptime}"
        );
    } else {
        let last_before_vt = gw_before.last().unwrap().0;
        let first_after_vt = gw_after.first().unwrap().0;
        assert!(
            last_before_vt < REBOOT_AT_US,
            "pre-reboot heartbeat must precede fault time"
        );
        assert!(
            first_after_vt >= REBOOT_AT_US + REBOOT_DOWNTIME_US,
            "post-reboot heartbeat must follow downtime"
        );
    }

    let pt_before: Vec<_> = pt.iter().filter(|&&(t, _)| t < REBOOT_AT_US).collect();
    let pt_during: Vec<_> = pt
        .iter()
        .filter(|&&(t, _)| (REBOOT_AT_US..REBOOT_AT_US + REBOOT_DOWNTIME_US).contains(&t))
        .collect();
    let pt_after: Vec<_> = pt
        .iter()
        .filter(|&&(t, _)| t >= REBOOT_AT_US + REBOOT_DOWNTIME_US)
        .collect();
    assert!(
        !pt_before.is_empty(),
        "sibling must heartbeat before gateway reboot (got {} powertrain heartbeats)",
        pt.len()
    );
    assert!(
        !pt_during.is_empty(),
        "sibling must keep heartbeating during gateway downtime"
    );
    assert!(
        !pt_after.is_empty(),
        "sibling must heartbeat after gateway reboot"
    );

    // Powertrain uptime is nondecreasing across the entire run — proves the
    // sibling never restarted (a reboot would drop uptime back near zero).
    for pair in pt.windows(2) {
        assert!(
            pair[1].1 >= pair[0].1,
            "powertrain uptime must be nondecreasing: {:?} -> {:?}",
            pair[0],
            pair[1]
        );
    }

    let last_before = *pt_before.last().unwrap();
    let first_after = *pt_after.first().unwrap();
    assert!(
        first_after.1 >= last_before.1,
        "powertrain uptime must continue across gateway reboot: before={last_before:?} after={first_after:?}"
    );
    assert!(
        first_after.1 > BOOT_UPTIME_CEILING_MS,
        "powertrain uptime must not return to boot range after gateway reboot, got {}",
        first_after.1
    );

    // FreeRTOS scheduler time and World virtual time are coupled but not
    // lock-step under multi-machine interleaving; require uptime to move
    // forward across the outage by at least one heartbeat period worth of
    // progress rather than matching vt delta exactly.
    let uptime_delta = first_after.1.saturating_sub(last_before.1);
    assert!(
        uptime_delta >= 50,
        "powertrain uptime must advance across gateway downtime by >= 50 ms, \
         got delta={uptime_delta} before={last_before:?} after={first_after:?}"
    );

    // No heartbeat gap larger than the expected period + tolerance (World µs).
    for pair in pt.windows(2) {
        let gap = pair[1].0.saturating_sub(pair[0].0);
        assert!(
            gap <= HEARTBEAT_GAP_TOLERANCE_US,
            "powertrain heartbeat gap {gap} µs exceeds tolerance {HEARTBEAT_GAP_TOLERANCE_US} µs \
             between {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
}

fn assert_reboot_invariants(world: &mut World, traces: &[String]) {
    let reset_begin = traces
        .iter()
        .filter(|l| l.contains("machine_reset_begin"))
        .count();
    let reset_boot = traces
        .iter()
        .filter(|l| l.contains("machine_reset_boot"))
        .count();
    assert_eq!(reset_begin, 1, "exactly one machine_reset_begin");
    assert_eq!(reset_boot, 1, "exactly one machine_reset_boot");

    // Exactly two gateway boots: initial factory boot + one post-reboot boot.
    // Match the exact user-u32 label so variants like `microcar_boot_gateway_diag`
    // cannot inflate the count.
    let gateway_boots = traces
        .iter()
        .filter(|l| l.contains("user-u32 \"microcar_boot_gateway\""))
        .count();
    assert_eq!(
        gateway_boots, 2,
        "gateway must boot exactly twice (initial + post-reboot), got {gateway_boots}"
    );
    assert!(
        traces.iter().any(|l| l.contains("gateway_mutex")),
        "gateway FreeRTOS primitives must be re-created after boot"
    );
    assert!(
        !traces.iter().any(|l| l.contains("fault:reboot")),
        "legacy fault:reboot marker must not appear"
    );
    assert!(
        !traces
            .iter()
            .any(|l| l.contains("[machine.2]") && l.contains("gateway_timeout")),
        "sibling powertrain must not observe gateway_timeout"
    );

    let gw = world.machine(1).expect("gateway present after reboot");
    assert_eq!(gw.id, 1);
    assert_eq!(gw.name, "gateway");
    assert_eq!(gw.rtos, RtosBackend::FreeRtos);
    assert!(
        gw.firmware_factory().is_some(),
        "firmware factory must survive restart"
    );

    let pt = world.machine(2).expect("powertrain sibling intact");
    assert_eq!(pt.id, 2);
    assert_eq!(pt.name, "powertrain");
    assert_eq!(pt.rtos, RtosBackend::FreeRtos);

    let flash = read_flash_marker(world);
    assert_eq!(
        flash, FLASH_MARKER,
        "persistent flash must survive gateway reboot"
    );

    let (tx_len, rx_len) = can_queue_lens(world);
    assert_eq!(
        tx_len, 0,
        "volatile CAN TX queue must be empty after reboot"
    );
    assert_eq!(
        rx_len, 0,
        "volatile CAN RX queue must be empty after reboot"
    );
}

fn run_once() {
    let mut world = build_world();
    write_flash_marker(&mut world);

    // Drain Trace v2 *and* human traces in phases straddling the reboot so
    // pre-reboot records (including the initial `microcar_boot_gateway`) are
    // not discarded with the reconstructed machine's fresh sinks.
    //
    // `drain_all_traces` does *not* clear sinks: collect human boot/reset
    // markers only through the post-reboot phase. A later human drain would
    // re-emit the reconstructed gateway's uncleared reset/boot lines.
    let mut recs: Vec<sim_core::TraceV2> = Vec::new();
    let mut traces: Vec<String> = Vec::new();

    world
        .run_until(REBOOT_AT_US - 1_000)
        .expect("run_until pre-reboot");
    recs.extend(world.drain_trace_v2());
    traces.extend(world.drain_all_traces());

    world
        .run_until(REBOOT_AT_US + REBOOT_DOWNTIME_US + 1_000)
        .expect("run_until through reboot");
    recs.extend(world.drain_trace_v2());
    traces.extend(world.drain_all_traces());

    world.run_until(2_000_000).expect("run_until to end");
    recs.extend(world.drain_trace_v2());

    assert_heartbeat_reboot_proof(&recs);
    assert_reboot_invariants(&mut world, &traces);
}

#[test]
fn gateway_reboot_preserves_flash_resets_volatile_once() {
    run_once();
}

#[test]
fn gateway_reboot_preserves_flash_resets_volatile_100x() {
    for i in 0..100 {
        run_once();
        if i % 20 == 19 {
            eprintln!("gateway_reboot_downtime: completed {} iterations", i + 1);
        }
    }
}

#[test]
fn gateway_reboot_toml_fixture_passes() {
    let scenario =
        Scenario::from_file("dogfood/b3_gateway_reboot_downtime.toml").expect("load b3 fixture");
    let mut world = scenario.build_world().expect("build");
    world.enable_owned_device_banks();
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
    scenario.schedule_faults_to(&mut world);
    world
        .run_until(scenario.duration_ms.unwrap() * 1000)
        .expect("run");
    let traces = world.drain_all_traces();
    let result = scenario.check_trace(traces).expect("check_trace");
    assert!(
        result.trace_match,
        "b3_gateway_reboot_downtime.toml expectations failed"
    );
}
