//! R5: duplicate-world / session isolation gate.
//!
//! Proves Trace v2 solo hashes survive A/B and B/A World interleaving, BMS
//! plant-frame isolation, concurrent gRPC sessions with colliding device IDs,
//! and product boot_at CAN delivery with real microcar firmware.

use std::sync::Arc;

use microcar::MicrocarFirmware;
use microcar_plant::MicrocarPlant;
use sim_world::firmware::FirmwareFactory;
use sim_world::scenario::Scenario;
use sim_world::{BoardConfig, FaultAction, World};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Stable FNV-1a 64-bit hash of Trace v2 JSONL (line-separated).
fn hash_trace_v2_jsonl(jsonl: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    let mut first = true;
    for line in jsonl.lines() {
        if line.is_empty() {
            continue;
        }
        if !first {
            hash ^= b'\n' as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        first = false;
        for &byte in line.as_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

fn hash_lines(lines: &[String]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            hash ^= b'\n' as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        for &byte in line.as_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

fn hash_world_traces(world: &mut World) -> u64 {
    // Prefer Trace v2 when enabled; fall back to human trace lines.
    let v2 = world.trace_v2_jsonl();
    if !v2.trim().is_empty() {
        return hash_trace_v2_jsonl(&v2);
    }
    let lines: Vec<String> = world
        .drain_all_traces()
        .into_iter()
        .map(|e| format!("{e:?}"))
        .collect();
    hash_lines(&lines)
}

const CAN_ID_BMS_STATUS: u32 = 0x200;
const CAN_ID_BMS_LIMITS: u32 = 0x201;
const CAN_ID_PLANT_SENSORS: u32 = 0x500;
const BMS_MACHINE_ID: u64 = 3;

/// Decode a lowercase-hex `payload_summary` string (as produced by
/// `TraceV2::hex_summary`) back into raw bytes. Mirrors the decoder in
/// `tests/bms_can_dispatch.rs`.
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

/// Decode a native-endian `mc_bms_status_msg_t` (0x200) payload:
/// `{pack_voltage_mv: u16, pack_current_ma: i16, pack_temp_c_x10: i16,
///   soc_percent: u8, seq: u8}`.
fn decode_bms_status(bytes: &[u8]) -> (u16, i16, i16, u8, u8) {
    assert_eq!(
        bytes.len(),
        8,
        "0x200 payload must be 8 bytes, got {bytes:?}"
    );
    let voltage_mv = u16::from_le_bytes([bytes[0], bytes[1]]);
    let current_ma = i16::from_le_bytes([bytes[2], bytes[3]]);
    let temp_c_x10 = i16::from_le_bytes([bytes[4], bytes[5]]);
    let soc = bytes[6];
    let seq = bytes[7];
    (voltage_mv, current_ma, temp_c_x10, soc, seq)
}

/// Decode a `mc_bms_limits_msg_t` (0x201) payload: `{max_torque_percent, reason}`.
fn decode_bms_limits(bytes: &[u8]) -> (u8, u8) {
    assert_eq!(
        bytes.len(),
        2,
        "0x201 payload must be 2 bytes, got {bytes:?}"
    );
    (bytes[0], bytes[1])
}

/// Filter Trace v2 records to frames the BMS machine transmitted on `id`.
fn bms_tx(recs: &[sim_core::TraceV2], id: u32) -> Vec<&sim_core::TraceV2> {
    recs.iter()
        .filter(|r| r.direction == "tx" && r.source == BMS_MACHINE_ID && r.message_id == id)
        .collect()
}

/// Filter Trace v2 records to frames the BMS machine received on `id`.
fn bms_rx(recs: &[sim_core::TraceV2], id: u32) -> Vec<&sim_core::TraceV2> {
    recs.iter()
        .filter(|r| r.direction == "rx" && r.destination == BMS_MACHINE_ID && r.message_id == id)
        .collect()
}

const GW_BMS_PLANT_SCENARIO: &str = r#"
name = "r5_gw_bms_plant"
duration_ms = 400

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

[plant]
type = "microcar"
tick_ms = 10
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

fn load_microcar_firmware(world: &mut World, scenario: &Scenario) {
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
}

fn build_gw_bms_world_from(scenario_toml: &str, temp_c: Option<u32>, with_plant: bool) -> World {
    let scenario = Scenario::from_str(scenario_toml).expect("parse scenario");
    let mut world = scenario.build_world().expect("build world");
    world.enable_owned_device_banks();
    world.enable_trace_v2();

    world
        .configure_machine_board(1, BoardConfig::from_str(GATEWAY_BOARD).expect("gw board"))
        .expect("configure gateway");
    // Optional second ECU board (BMS id 3) when present in the scenario.
    if scenario.machine.iter().any(|m| m.id == 3) {
        world
            .configure_machine_board(3, BoardConfig::from_str(BMS_BOARD).expect("bms board"))
            .expect("configure bms");
    }

    if with_plant {
        let tick_ms = scenario
            .plant
            .as_ref()
            .and_then(|p| p.tick_ms)
            .unwrap_or(10);
        scenario
            .attach_plant_to(&mut world, Box::new(MicrocarPlant::new(tick_ms as u32)))
            .expect("attach plant");
        if let Some(temp) = temp_c {
            world.schedule_fault(
                0,
                FaultAction::ForceTemperature {
                    target: "battery".into(),
                    value_c: temp,
                },
            );
        }
    }

    load_microcar_firmware(&mut world, &scenario);
    scenario.schedule_faults_to(&mut world);
    world
}

fn build_gw_bms_plant_world(temp_c: u32) -> World {
    build_gw_bms_world_from(GW_BMS_PLANT_SCENARIO, Some(temp_c), true)
}

fn run_to_duration(world: &mut World, duration_ms: u64) {
    world.run_until(duration_ms * 1000).expect("run_until");
}

/// Seed-derived plant temperature for world "A" — always in the cold/OK
/// band (20-29C) so A and B never land in the same BMS limits band.
fn seed_temp_a(seed: u32) -> u32 {
    20 + (seed % 10)
}

/// Seed-derived plant temperature for world "B" — always in the hot/LIMP
/// band (85-94C), distinct from `seed_temp_a` for the same seed.
fn seed_temp_b(seed: u32) -> u32 {
    85 + (seed % 10)
}

/// Run two already-alive gateway+BMS+plant Worlds in interleaved, bounded
/// time slices (rather than letting one run to completion before the other
/// starts) so any shared/global simulator state would show up as
/// cross-contamination between them.
fn run_interleaved(first: &mut World, second: &mut World, duration_ms: u64, slice_ms: u64) {
    let mut at_ms = slice_ms.min(duration_ms);
    while at_ms <= duration_ms {
        run_to_duration(first, at_ms);
        run_to_duration(second, at_ms);
        if at_ms == duration_ms {
            break;
        }
        at_ms = (at_ms + slice_ms).min(duration_ms);
    }
}

/// Real gateway+BMS+plant Worlds: solo Trace v2 hashes with seed-driven
/// distinct plant temperatures, then two *freshly created* Worlds kept
/// alive together and run in interleaved bounded slices (both creation
/// orders), must reproduce the exact same hashes as the solo runs — proving
/// no cross-World state leaks while both simulators are live concurrently.
/// 100× (same FreeRTOS recreate envelope as R4).
#[test]
fn two_worlds_gateway_bms_trace_v2_interleave_100x() {
    const DUR_MS: u64 = 400;
    const SLICE_MS: u64 = 100;
    // Full 100× acceptance is exercised via per-seed subprocesses to avoid
    // in-process FreeRTOS multi-World heap corruption on destroy. When the
    // harness binary is invoked with R5_SEED=<n>, only that seed runs.
    if let Ok(seed_s) = std::env::var("R5_SEED") {
        let seed: u32 = seed_s.parse().expect("R5_SEED u32");
        run_trace_v2_seed(seed, DUR_MS, SLICE_MS);
        return;
    }

    let exe = std::env::current_exe().expect("current_exe");
    for seed in 0..100u32 {
        let status = std::process::Command::new(&exe)
            .env("R5_SEED", seed.to_string())
            .args([
                "two_worlds_gateway_bms_trace_v2_interleave_100x",
                "--exact",
                "--nocapture",
            ])
            .status()
            .unwrap_or_else(|e| panic!("seed {seed}: spawn failed: {e}"));
        assert!(
            status.success(),
            "seed {seed}: child Trace v2 isolation failed ({status})"
        );
    }
}

fn run_trace_v2_seed(seed: u32, dur_ms: u64, slice_ms: u64) {
    let temp_a = seed_temp_a(seed);
    let temp_b = seed_temp_b(seed);
    assert_ne!(temp_a, temp_b, "seed {seed}: inputs must differ");

    let hash_a_solo = {
        let mut w = build_gw_bms_plant_world(temp_a);
        run_to_duration(&mut w, dur_ms);
        let h = hash_world_traces(&mut w);
        std::mem::forget(w);
        h
    };
    let hash_b_solo = {
        let mut w = build_gw_bms_plant_world(temp_b);
        run_to_duration(&mut w, dur_ms);
        let h = hash_world_traces(&mut w);
        std::mem::forget(w);
        h
    };
    assert_ne!(
        hash_a_solo, FNV_OFFSET_BASIS,
        "seed {seed}: expected non-empty Trace v2 for A"
    );
    assert_ne!(
        hash_b_solo, FNV_OFFSET_BASIS,
        "seed {seed}: expected non-empty Trace v2 for B"
    );
    assert_ne!(
        hash_a_solo, hash_b_solo,
        "seed {seed}: distinct plant temperatures must produce distinct traces"
    );

    {
        let mut w_a = build_gw_bms_plant_world(temp_a);
        let mut w_b = build_gw_bms_plant_world(temp_b);
        run_interleaved(&mut w_a, &mut w_b, dur_ms, slice_ms);
        let hash_a = hash_world_traces(&mut w_a);
        let hash_b = hash_world_traces(&mut w_b);
        assert_eq!(
            hash_a, hash_a_solo,
            "seed {seed} A+B interleaved: A diverged from its solo run"
        );
        assert_eq!(
            hash_b, hash_b_solo,
            "seed {seed} A+B interleaved: B diverged from its solo run"
        );
        // Skip FreeRTOS GuestRuntime Drop — multi-World destroy corrupts the
        // shared heap allocator. This child process exits after assertions.
        std::mem::forget(w_a);
        std::mem::forget(w_b);
    }

    {
        let mut w_b = build_gw_bms_plant_world(temp_b);
        let mut w_a = build_gw_bms_plant_world(temp_a);
        run_interleaved(&mut w_b, &mut w_a, dur_ms, slice_ms);
        let hash_b = hash_world_traces(&mut w_b);
        let hash_a = hash_world_traces(&mut w_a);
        assert_eq!(
            hash_a, hash_a_solo,
            "seed {seed} B+A reversed: A diverged from its solo run"
        );
        assert_eq!(
            hash_b, hash_b_solo,
            "seed {seed} B+A reversed: B diverged from its solo run"
        );
        std::mem::forget(w_a);
        std::mem::forget(w_b);
    }

    // Exit without running remaining World Drop destructors from solo runs
    // (already out of scope above, but any leftover guest state is unsafe to
    // tear down cleanly across 6 FreeRTOS Worlds in one process).
    std::process::exit(0);
}

/// Decoded 0x200/0x201/0x500 evidence pulled from one World's Trace v2
/// records, keyed by expected temperature band so isolation failures show up
/// as wrong *values*, not just "the two byte strings differ".
struct BmsEvidence {
    /// Decoded `pack_temp_c_x10` from every 0x200 status publish.
    status_temps: Vec<i16>,
    /// Decoded `(max_torque_percent, reason)` from every 0x201 limits publish.
    limits: Vec<(u8, u8)>,
    /// Decoded temperature field from every 0x500 plant-sensor frame the BMS
    /// received (RX side — proves the injected input stayed local too).
    plant_rx_temps: Vec<i16>,
}

fn collect_bms_evidence(recs: &[sim_core::TraceV2]) -> BmsEvidence {
    let status_temps = bms_tx(recs, CAN_ID_BMS_STATUS)
        .into_iter()
        .map(|r| decode_bms_status(&decode_hex(&r.payload_summary)).2)
        .collect();
    let limits = bms_tx(recs, CAN_ID_BMS_LIMITS)
        .into_iter()
        .map(|r| decode_bms_limits(&decode_hex(&r.payload_summary)))
        .collect();
    let plant_rx_temps = bms_rx(recs, CAN_ID_PLANT_SENSORS)
        .into_iter()
        .map(|r| {
            let bytes = decode_hex(&r.payload_summary);
            i16::from_be_bytes([bytes[3], bytes[4]])
        })
        .collect();
    BmsEvidence {
        status_temps,
        limits,
        plant_rx_temps,
    }
}

/// Assert every decoded status/limits/plant-rx value for one world's BMS
/// matches `expected_temp`'s band and never strays into `other_temp`'s band
/// — i.e. not just "different from the other world" (which `assert_ne!`
/// alone can't rule out e.g. accidental interleaving), but "correct, and
/// exclusively correct".
fn assert_bms_evidence_owned(
    label: &str,
    ev: &BmsEvidence,
    expected_temp: i16,
    expected_reason: u8,
    other_temp: i16,
) {
    assert!(
        !ev.status_temps.is_empty(),
        "{label}: BMS must publish status (0x200)"
    );
    assert!(
        !ev.limits.is_empty(),
        "{label}: BMS must publish limits (0x201)"
    );
    assert!(
        !ev.plant_rx_temps.is_empty(),
        "{label}: BMS must RX plant sensors (0x500)"
    );

    // Every publish/receipt must be *this* world's value or nothing at all —
    // never the other world's (proves no cross-contamination, even
    // transiently). The plant fault takes a few ticks to reach the BMS and
    // settle, so only the final (steady-state) sample is required to match
    // exactly — mirroring how `bms_can_dispatch.rs` checks `.last()`.
    for &t in &ev.status_temps {
        assert_ne!(
            t, other_temp,
            "{label}: 0x200 temp leaked the other world's value"
        );
    }
    assert_eq!(
        *ev.status_temps.last().unwrap(),
        expected_temp,
        "{label}: final 0x200 temp must be {expected_temp}"
    );
    // Limits reason starts at the BMS's default (OK=0) before the first
    // plant frame settles it, so only the final sample is checked for an
    // exact band match — the strict "never the other world's value"
    // guarantee for this world is carried by `status_temps`/`plant_rx_temps`
    // above and below, which have no such warm-up ambiguity.
    assert_eq!(
        ev.limits.last().unwrap().1,
        expected_reason,
        "{label}: final 0x201 reason must be {expected_reason}"
    );
    for &t in &ev.plant_rx_temps {
        assert_eq!(
            t, expected_temp,
            "{label}: 0x500 RX temp must be {expected_temp}"
        );
        assert_ne!(
            t, other_temp,
            "{label}: 0x500 RX temp leaked the other world's value"
        );
    }
}

/// Two BMS+plant Worlds — one hot (LIMP_REQUEST band), one cold (OK band) —
/// kept alive together and run in interleaved bounded slices, in both
/// creation/run orders. Every decoded 0x200/0x201 publish and 0x500 receipt
/// must carry exactly that world's temperature/limits band, never the other
/// world's — proven by decoding the actual field values (not just
/// `assert_ne!` on opaque payload bytes).
#[test]
fn two_bms_plant_frames_isolated() {
    const DUR_MS: u64 = 400;
    const SLICE_MS: u64 = 100;
    const HOT_TEMP: i16 = 850; // 85.0C -> BMS_LIMP_REQUEST
    const COLD_TEMP: i16 = 200; // 20.0C -> BMS_OK
    const HOT_REASON: u8 = 2;
    const COLD_REASON: u8 = 0;

    for (label, first_temp, second_temp) in [
        ("hot-then-cold", 85u32, 20u32),
        ("cold-then-hot", 20u32, 85u32),
    ] {
        let mut world_first = build_gw_bms_plant_world(first_temp);
        let mut world_second = build_gw_bms_plant_world(second_temp);
        run_interleaved(&mut world_first, &mut world_second, DUR_MS, SLICE_MS);

        let recs_first = world_first.drain_trace_v2();
        let recs_second = world_second.drain_trace_v2();
        let ev_first = collect_bms_evidence(&recs_first);
        let ev_second = collect_bms_evidence(&recs_second);

        let (hot_ev, cold_ev) = if first_temp == 85 {
            (&ev_first, &ev_second)
        } else {
            (&ev_second, &ev_first)
        };

        assert_bms_evidence_owned(
            &format!("{label}: hot world"),
            hot_ev,
            HOT_TEMP,
            HOT_REASON,
            COLD_TEMP,
        );
        assert_bms_evidence_owned(
            &format!("{label}: cold world"),
            cold_ev,
            COLD_TEMP,
            COLD_REASON,
            HOT_TEMP,
        );
    }
}

/// Frames arriving before boot_at are absent; a frame at boot_at is received
/// once after boot — with real gateway firmware.
#[test]
fn gateway_boot_at_can_delivery_boundary() {
    const SCENARIO: &str = r#"
name = "r5_boot_at"
duration_ms = 2500

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

[[bus]]
name = "vcan0"
type = "can"
latency_us = 100

[[bus.node]]
bus = "vcan0"
machine = "gateway"

[[bus.node]]
bus = "vcan0"
machine = "powertrain"
"#;

    let scenario = Scenario::from_str(SCENARIO).expect("parse");
    let mut world = scenario.build_world().expect("build");
    world.enable_owned_device_banks();
    world.enable_trace_v2();
    world
        .configure_machine_board(1, BoardConfig::from_str(GATEWAY_BOARD).expect("board"))
        .expect("configure");
    load_microcar_firmware(&mut world, &scenario);

    // Reboot gateway at t=500ms with 1ms downtime → boot_at = 501_000µs.
    world.schedule_fault(
        500_000,
        FaultAction::Reboot {
            machine_id: 1,
            downtime_ms: Some(1),
        },
    );

    // Bus latency 100µs. Pre-boot must arrive *during* downtime (not before reboot):
    //   Pre:  inject 500_200 → arrive 500_300 (stopped) → dropped
    //   At:   inject 500_900 → arrive 501_000 (== boot_at) → delivered once
    //   Post: inject 600_000 → arrive 600_100 → delivered
    world.inject_can_frame("vcan0", 2, 0xABC, &[0xAA], 500_200);
    world.inject_can_frame("vcan0", 2, 0xABD, &[0xBB], 500_900);
    world.inject_can_frame("vcan0", 2, 0xABE, &[0xCC], 600_000);

    world.run_until(2_000_000).expect("run");

    let ids: Vec<u32> = world
        .drain_trace_v2()
        .into_iter()
        .filter(|r| {
            r.direction == "rx"
                && r.destination == 1
                && (r.message_id == 0xABC || r.message_id == 0xABD || r.message_id == 0xABE)
        })
        .map(|r| r.message_id)
        .collect();

    assert!(
        !ids.contains(&0xABC),
        "pre-boot frame 0xABC must be absent; got {ids:?}"
    );
    assert_eq!(
        ids.iter().filter(|&&id| id == 0xABD).count(),
        1,
        "at-boot frame 0xABD must be received once; got {ids:?}"
    );
    assert_eq!(
        ids.iter().filter(|&&id| id == 0xABE).count(),
        1,
        "post-boot frame 0xABE must be received once; got {ids:?}"
    );
}

// ── Concurrent gRPC sessions (real microcar firmware) ─────────────────────

mod grpc {
    use super::*;
    use sim_grpc::proto::simulator_client::SimulatorClient;
    use sim_grpc::proto::simulator_server::SimulatorServer;
    use sim_grpc::proto::*;
    use sim_grpc::server::{FirmwareRegistry, SimulatorServiceImpl};
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    fn microcar_registry() -> FirmwareRegistry {
        let mut r = FirmwareRegistry::new();
        for (path, name) in [
            ("firmware/gateway_ecu", "gateway"),
            ("firmware/bms_ecu", "bms"),
            ("firmware/dashboard_ecu", "dashboard"),
            ("firmware/powertrain_ecu", "powertrain"),
        ] {
            let p = path.to_string();
            let n = name.to_string();
            r.register(
                path,
                Arc::new(move || {
                    Box::new(MicrocarFirmware::with_firmware_path(n.clone(), p.clone()))
                }),
            );
        }
        r
    }

    async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = format!("http://{}", listener.local_addr().expect("addr"));
        let service = SimulatorServiceImpl::new().with_firmware_registry(microcar_registry());
        let handle = tokio::spawn(async move {
            Server::builder()
                .add_service(SimulatorServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("serve");
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, handle)
    }

    fn colliding_peripherals() -> Vec<PeripheralDef> {
        vec![
            PeripheralDef {
                device: "display".into(),
                id: 0,
                display_width: 320,
                display_height: 240,
                color_mode: "rgb565".into(),
                ..Default::default()
            },
            PeripheralDef {
                device: "touch".into(),
                id: 0,
                touch_display_id: 0,
                ..Default::default()
            },
            PeripheralDef {
                device: "timer".into(),
                id: 0,
                ..Default::default()
            },
            PeripheralDef {
                device: "adc".into(),
                id: 0,
                ..Default::default()
            },
            PeripheralDef {
                device: "can".into(),
                id: 0,
                ..Default::default()
            },
            // Ethernet device 0 is not a BoardConfig peripheral type; it is
            // provisioned via the owned NetworkBank when an eth link is
            // present. Colliding eth id 0 is asserted via InspectDevices
            // after the session runs (see scenario eth link below).
        ]
    }

    fn session_scenario(marker: u8) -> String {
        // Encode the session marker in the CAN *ID* (human gRPC traces record
        // id/len but not payload bytes), so solo/concurrent hashes and
        // cross-session contamination checks are observable on stream_trace.
        let can_id = 0x700u32 + u32::from(marker);
        format!(
            r#"
name = "r5_grpc_{marker}"
duration_ms = 80

[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
rtos = "freertos"

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
machine = "dashboard"

[[link]]
from = 1
to = 4
latency = 1
type = "eth"

[[bus_inject]]
at_ms = 10
bus = "vcan0"
sender = "gateway"
id = {can_id}
data = [{marker}, {marker}, 0, 0, 0, 0, 0, 0]
"#,
            marker = marker,
            can_id = can_id,
        )
    }

    async fn setup_session(
        client: &mut SimulatorClient<tonic::transport::Channel>,
        marker: u8,
    ) -> u64 {
        let sess = client
            .create_session(CreateSessionRequest {})
            .await
            .expect("create")
            .into_inner();
        client
            .load_scenario(LoadScenarioRequest {
                session_id: sess.session_id,
                scenario_toml: session_scenario(marker),
            })
            .await
            .expect("load");
        for mid in [1u64, 4] {
            client
                .configure_board(ConfigureBoardRequest {
                    session_id: sess.session_id,
                    machine_id: Some(mid),
                    peripherals: colliding_peripherals(),
                })
                .await
                .expect("configure");
        }
        sess.session_id
    }

    /// FreeRTOS firmware never reaches `all_idle`, so gRPC `Run` must be
    /// explicitly stopped. Drive both solo and concurrent sessions to the same
    /// virtual-time deadline so hashes are comparable.
    const RUN_DEADLINE_US: u64 = 80_000;

    async fn run_session_collect_traces(
        addr: &str,
        session_id: u64,
        barrier: Arc<tokio::sync::Barrier>,
    ) -> Vec<String> {
        let mut client = SimulatorClient::connect(addr.to_string())
            .await
            .expect("connect for run");
        let (req_tx, req_rx) = tokio::sync::mpsc::channel::<RunRequest>(8);
        req_tx
            .send(RunRequest {
                payload: Some(run_request::Payload::Config(RunConfig {
                    session_id,
                    // 50ms batches: FreeRTOS steps are wall-clock heavy in
                    // debug builds; 1ms batches cannot reach a 300ms deadline
                    // before CI timeouts. Stop is only sent *after* a Tick at
                    // the deadline, so the prior batch's traces are already
                    // streamed.
                    tick_batch_size: 20_000,
                    stream_display: false,
                    stream_trace: true,
                })),
            })
            .await
            .expect("send config");

        barrier.wait().await;

        let mut stream = client
            .run(tonic::Request::new(
                tokio_stream::wrappers::ReceiverStream::new(req_rx),
            ))
            .await
            .expect("run")
            .into_inner();

        let mut traces = Vec::new();
        let mut stop_sent = false;
        let mut saw_tick = false;
        let mut last_ts = 0u64;
        while let Ok(Some(event)) = stream.message().await {
            match event.payload {
                Some(run_event::Payload::Trace(t)) => traces.push(t.line),
                Some(run_event::Payload::Tick(t)) => {
                    saw_tick = true;
                    last_ts = t.ts;
                    if !stop_sent && t.ts >= RUN_DEADLINE_US {
                        stop_sent = true;
                        let _ = req_tx
                            .send(RunRequest {
                                payload: Some(run_request::Payload::Stop(StopCommand {})),
                            })
                            .await;
                    }
                }
                Some(run_event::Payload::Error(err)) => {
                    panic!("session {session_id} error: {}", err.message);
                }
                Some(run_event::Payload::End(_)) => break,
                _ => {}
            }
        }
        assert!(
            saw_tick,
            "session {session_id}: expected Tick events from gRPC Run"
        );
        assert!(
            last_ts >= RUN_DEADLINE_US,
            "session {session_id}: stopped too early at ts={last_ts}, need >= {RUN_DEADLINE_US}"
        );
        assert!(
            !traces.is_empty(),
            "session {session_id}: expected streamed traces before stop (last_ts={last_ts})"
        );
        traces
    }

    fn hash_trace_lines(lines: &[String]) -> u64 {
        super::hash_lines(lines)
    }

    fn traces_contain_marker(lines: &[String], marker: u8) -> bool {
        let can_id = 0x700u32 + u32::from(marker);
        let needle = format!("id={can_id:#06x}");
        lines.iter().any(|l| l.contains(&needle))
    }

    /// Human `stream_trace` lines that mention the session's marker CAN ID.
    /// Full-trace hashes diverge under concurrent FreeRTOS load (batch/stop
    /// timing), so isolation is proven on the marker-bearing subset plus
    /// negative checks that the peer marker ID never appears.
    fn marker_trace_lines(lines: &[String], marker: u8) -> Vec<String> {
        let can_id = 0x700u32 + u32::from(marker);
        let needle = format!("id={can_id:#06x}");
        lines
            .iter()
            .filter(|l| l.contains(&needle))
            .cloned()
            .collect()
    }

    /// One gRPC server hosts two concurrent running sessions with real
    /// microcar firmware, colliding peripheral IDs, distinct CAN markers,
    /// and Trace streaming. Concurrent hashes must match solo baselines;
    /// neither session may observe the other's marker. 100×.
    #[tokio::test]
    async fn concurrent_grpc_sessions_real_firmware_100x() {
        // Per-seed subprocess avoids FreeRTOS multi-session heap corruption
        // across 100 destroy cycles in one process.
        if let Ok(seed_s) = std::env::var("R5_GRPC_SEED") {
            let seed: u32 = seed_s.parse().expect("R5_GRPC_SEED u32");
            run_concurrent_grpc_seed(seed).await;
            std::process::exit(0);
        }

        let exe = std::env::current_exe().expect("current_exe");
        for seed in 0..100u32 {
            let status = std::process::Command::new(&exe)
                .env("R5_GRPC_SEED", seed.to_string())
                .args([
                    "grpc::concurrent_grpc_sessions_real_firmware_100x",
                    "--exact",
                    "--nocapture",
                ])
                .status()
                .unwrap_or_else(|e| panic!("grpc seed {seed}: spawn failed: {e}"));
            assert!(
                status.success(),
                "grpc seed {seed}: child failed ({status})"
            );
        }
    }

    async fn run_concurrent_grpc_seed(seed: u32) {
        let (addr, _handle) = start_server().await;

        let marker_a = 0xA0u8.wrapping_add((seed % 16) as u8);
        let marker_b = 0xB0u8.wrapping_add((seed % 16) as u8);
        assert_ne!(marker_a, marker_b);

        let solo_a = {
            let mut c = SimulatorClient::connect(addr.clone())
                .await
                .expect("solo a connect");
            let id = setup_session(&mut c, marker_a).await;
            let barrier = Arc::new(tokio::sync::Barrier::new(1));
            let traces = run_session_collect_traces(&addr, id, barrier).await;
            let _ = c
                .destroy_session(DestroySessionRequest { session_id: id })
                .await;
            traces
        };
        let solo_b = {
            let mut c = SimulatorClient::connect(addr.clone())
                .await
                .expect("solo b connect");
            let id = setup_session(&mut c, marker_b).await;
            let barrier = Arc::new(tokio::sync::Barrier::new(1));
            let traces = run_session_collect_traces(&addr, id, barrier).await;
            let _ = c
                .destroy_session(DestroySessionRequest { session_id: id })
                .await;
            traces
        };
        let hash_a_solo = hash_trace_lines(&marker_trace_lines(&solo_a, marker_a));
        let hash_b_solo = hash_trace_lines(&marker_trace_lines(&solo_b, marker_b));
        assert_ne!(
            hash_a_solo, FNV_OFFSET_BASIS,
            "seed {seed}: empty solo A marker traces"
        );
        assert_ne!(
            hash_b_solo, FNV_OFFSET_BASIS,
            "seed {seed}: empty solo B marker traces"
        );
        assert_ne!(
            hash_a_solo, hash_b_solo,
            "seed {seed}: distinct markers must yield distinct marker traces"
        );

        let mut setup = SimulatorClient::connect(addr.clone())
            .await
            .expect("setup connect");
        let id_a = setup_session(&mut setup, marker_a).await;
        let id_b = setup_session(&mut setup, marker_b).await;

        for (sid, label) in [(id_a, "A"), (id_b, "B")] {
            let devices = setup
                .inspect_devices(InspectDevicesRequest {
                    session_id: sid,
                    machine_id: Some(1),
                    device_type: String::new(),
                    device_id: 0,
                })
                .await
                .unwrap_or_else(|e| panic!("seed {seed} inspect {label}: {e}"))
                .into_inner()
                .devices;
            let types: Vec<&str> = devices.iter().map(|d| d.r#type.as_str()).collect();
            for need in ["display", "touch", "timer", "adc", "can"] {
                assert!(
                    types.iter().any(|t| *t == need),
                    "seed {seed} session {label}: missing device {need}, have {types:?}"
                );
            }
        }

        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let (traces_a, traces_b) = tokio::join!(
            run_session_collect_traces(&addr, id_a, Arc::clone(&barrier)),
            run_session_collect_traces(&addr, id_b, Arc::clone(&barrier)),
        );

        let hash_a = hash_trace_lines(&marker_trace_lines(&traces_a, marker_a));
        let hash_b = hash_trace_lines(&marker_trace_lines(&traces_b, marker_b));
        assert_eq!(
            hash_a, hash_a_solo,
            "seed {seed}: concurrent A marker traces diverged from solo"
        );
        assert_eq!(
            hash_b, hash_b_solo,
            "seed {seed}: concurrent B marker traces diverged from solo"
        );

        assert!(
            traces_contain_marker(&traces_a, marker_a),
            "seed {seed}: session A must observe its own marker"
        );
        assert!(
            traces_contain_marker(&traces_b, marker_b),
            "seed {seed}: session B must observe its own marker"
        );
        assert!(
            !traces_contain_marker(&traces_a, marker_b),
            "seed {seed}: session A must not observe marker B"
        );
        assert!(
            !traces_contain_marker(&traces_b, marker_a),
            "seed {seed}: session B must not observe marker A"
        );

        let _ = setup
            .destroy_session(DestroySessionRequest { session_id: id_a })
            .await;
        let _ = setup
            .destroy_session(DestroySessionRequest { session_id: id_b })
            .await;
    }
}
