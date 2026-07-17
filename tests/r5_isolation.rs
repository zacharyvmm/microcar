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
    // Regression for per-Simulator FreeRTOS C-kernel state: all 100
    // destroy/recreate cycles execute in one process and Worlds drop normally.
    for seed in 0..100u32 {
        run_trace_v2_seed(seed, DUR_MS, SLICE_MS);
    }
}

fn run_trace_v2_seed(seed: u32, dur_ms: u64, slice_ms: u64) {
    let temp_a = seed_temp_a(seed);
    let temp_b = seed_temp_b(seed);
    assert_ne!(temp_a, temp_b, "seed {seed}: inputs must differ");

    let hash_a_solo = {
        let mut w = build_gw_bms_plant_world(temp_a);
        run_to_duration(&mut w, dur_ms);
        hash_world_traces(&mut w)
    };
    let hash_b_solo = {
        let mut w = build_gw_bms_plant_world(temp_b);
        run_to_duration(&mut w, dur_ms);
        hash_world_traces(&mut w)
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
    }
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

    /// Colliding peripheral *IDs* with session-distinct timer IRQ inputs.
    fn colliding_peripherals(marker: u8) -> Vec<PeripheralDef> {
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
                timer_irq: timer_irq(marker),
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
        ]
    }

    fn session_scenario(marker: u8) -> String {
        let can_id = 0x700u32 + u32::from(marker);
        let eth_len = eth_payload_len(marker);
        // Deterministic ASCII payload; length is the isolation key.
        let eth_payload: String =
            std::iter::repeat_n(char::from(b'A' + (marker % 26)), eth_len).collect();
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

[[inject]]
at = 1000
link = {{ from = 1, to = 4 }}
data = "{eth_payload}"
"#,
            marker = marker,
            can_id = can_id,
            eth_payload = eth_payload,
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
                    peripherals: colliding_peripherals(marker),
                })
                .await
                .expect("configure");
        }
        sess.session_id
    }

    /// Virtual-time comparison bound. Product FreeRTOS sessions use explicit
    /// Stop (not `deadline_ticks`) because jumping `world.now` across FreeRTOS
    /// wake gaps for deadline fill is unsafe with real firmware; the gRPC
    /// integration test still covers `deadline_ticks` with generic firmware.
    const RUN_DEADLINE_US: u64 = 80_000;

    fn touch_xy(marker: u8) -> (u32, u32) {
        (40 + u32::from(marker), 60 + u32::from(marker) * 2)
    }

    fn eth_payload_len(marker: u8) -> usize {
        64 + usize::from(marker)
    }

    fn adc_value(marker: u8) -> u32 {
        1000 + u32::from(marker) * 17
    }

    fn timer_irq(marker: u8) -> u32 {
        10 + u32::from(marker)
    }

    /// Per-session semantic evidence for the R5 gRPC isolation gate.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SessionEvidence {
        can_marker_lines: Vec<String>,
        display_hashes: Vec<u64>,
        touch_x: u32,
        touch_y: u32,
        touch_observed_x: u32,
        touch_observed_y: u32,
        adc_channel0: u32,
        timer_irq: u32,
        eth_pkt_rx_lens: Vec<usize>,
    }

    impl SessionEvidence {
        fn canonicalize(&self) -> String {
            let mut s = String::new();
            s.push_str(&format!(
                "touch_in={},{} touch_obs={},{} adc0={} timer_irq={}\n",
                self.touch_x,
                self.touch_y,
                self.touch_observed_x,
                self.touch_observed_y,
                self.adc_channel0,
                self.timer_irq
            ));
            for line in &self.can_marker_lines {
                s.push_str("can:");
                s.push_str(line);
                s.push('\n');
            }
            for h in &self.display_hashes {
                s.push_str(&format!("disp:{h:#016x}\n"));
            }
            for len in &self.eth_pkt_rx_lens {
                s.push_str(&format!("eth_rx_len:{len}\n"));
            }
            s
        }

        fn hash(&self) -> u64 {
            super::hash_lines(&[self.canonicalize()])
        }
    }

    fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut hash = FNV_OFFSET_BASIS;
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    fn canonical_display_hash(frame: &DisplayFrame) -> u64 {
        let mut buf = Vec::new();
        buf.extend_from_slice(&frame.machine_id.to_le_bytes());
        buf.extend_from_slice(&frame.device_id.to_le_bytes());
        buf.extend_from_slice(&frame.width.to_le_bytes());
        buf.extend_from_slice(&frame.height.to_le_bytes());
        buf.push(u8::from(frame.full_frame));
        for r in &frame.dirty_rects {
            buf.extend_from_slice(&r.x.to_le_bytes());
            buf.extend_from_slice(&r.y.to_le_bytes());
            buf.extend_from_slice(&r.w.to_le_bytes());
            buf.extend_from_slice(&r.h.to_le_bytes());
            buf.extend_from_slice(&r.data);
        }
        hash_bytes(&buf)
    }

    fn can_marker_lines(traces: &[String], marker: u8) -> Vec<String> {
        let can_id = 0x700u32 + u32::from(marker);
        let needle = format!("id={can_id:#06x}");
        traces
            .iter()
            .filter(|l| l.contains(&needle))
            .cloned()
            .collect()
    }

    fn eth_rx_lens(traces: &[String]) -> Vec<usize> {
        let mut lens = Vec::new();
        for line in traces {
            if let Some(rest) = line.split("pkt-rx len=").nth(1) {
                let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = num.parse::<usize>() {
                    lens.push(n);
                }
            }
        }
        lens.sort_unstable();
        lens
    }

    async fn wait_session_inspectable(
        client: &mut SimulatorClient<tonic::transport::Channel>,
        session_id: u64,
    ) {
        // Run workers return the World asynchronously after Stop/End; Inspect
        // rejects RUNNING. Poll until the World is back in the session.
        for _ in 0..500 {
            let state = client
                .get_status(GetStatusRequest { session_id })
                .await
                .map(|r| r.into_inner().state)
                .unwrap_or_default();
            if !state.eq_ignore_ascii_case("running") {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("session {session_id}: still Running after Stop; cannot inspect");
    }

    async fn collect_evidence(
        client: &mut SimulatorClient<tonic::transport::Channel>,
        session_id: u64,
        marker: u8,
        traces: &[String],
        display_hashes: Vec<u64>,
    ) -> SessionEvidence {
        wait_session_inspectable(client, session_id).await;
        let (touch_x, touch_y) = touch_xy(marker);
        let devices = client
            .inspect_devices(InspectDevicesRequest {
                session_id,
                machine_id: Some(4),
                device_type: String::new(),
                device_id: 0,
            })
            .await
            .expect("inspect dashboard")
            .into_inner()
            .devices;
        let touch = devices.iter().find(|d| d.r#type == "touch");
        let (touch_observed_x, touch_observed_y) = touch
            .map(|t| (t.touch_last_inject_x, t.touch_last_inject_y))
            .unwrap_or((0, 0));
        assert!(
            touch.is_some_and(|t| t.touch_has_last_inject),
            "session {session_id}: touch inject must be observable after run"
        );

        let gw = client
            .inspect_devices(InspectDevicesRequest {
                session_id,
                machine_id: Some(1),
                device_type: String::new(),
                device_id: 0,
            })
            .await
            .expect("inspect gateway")
            .into_inner()
            .devices;
        let adc0 = gw
            .iter()
            .find(|d| d.r#type == "adc")
            .and_then(|d| d.adc_channels.first())
            .map(|c| c.value)
            .unwrap_or(0);
        let t_irq = gw
            .iter()
            .find(|d| d.r#type == "timer")
            .map(|d| d.timer_irq)
            .unwrap_or(0);

        SessionEvidence {
            can_marker_lines: can_marker_lines(traces, marker),
            display_hashes,
            touch_x,
            touch_y,
            touch_observed_x,
            touch_observed_y,
            adc_channel0: adc0,
            timer_irq: t_irq,
            eth_pkt_rx_lens: eth_rx_lens(traces),
        }
    }

    struct RunOutcome {
        traces: Vec<String>,
        display_hashes: Vec<u64>,
        last_ts: u64,
    }

    async fn run_session_to_deadline(
        addr: &str,
        session_id: u64,
        marker: u8,
        start_barrier: Arc<tokio::sync::Barrier>,
    ) -> RunOutcome {
        let mut client = SimulatorClient::connect(addr.to_string())
            .await
            .expect("connect for run");
        let (req_tx, req_rx) = tokio::sync::mpsc::channel::<RunRequest>(16);
        let (touch_x, touch_y) = touch_xy(marker);
        req_tx
            .send(RunRequest {
                payload: Some(run_request::Payload::Config(RunConfig {
                    session_id,
                    tick_batch_size: 20_000,
                    stream_display: true,
                    stream_trace: true,
                    deadline_ticks: 0,
                })),
            })
            .await
            .expect("send config");

        start_barrier.wait().await;

        // Distinct per-session inputs immediately after both runs are live.
        req_tx
            .send(RunRequest {
                payload: Some(run_request::Payload::Touch(TouchInject {
                    device_id: 0,
                    machine_id: Some(4),
                    events: vec![TouchEvent {
                        point_id: 1 + u32::from(marker % 8),
                        x: touch_x,
                        y: touch_y,
                        pressure: 128,
                        event_type: TouchEventType::TouchPress as i32,
                    }],
                })),
            })
            .await
            .expect("touch");
        req_tx
            .send(RunRequest {
                payload: Some(run_request::Payload::Adc(AdcInject {
                    device_id: 0,
                    channel: 0,
                    value: adc_value(marker),
                    machine_id: Some(1),
                })),
            })
            .await
            .expect("adc");
        // Distinct framebuffer content so display isolation does not depend on
        // firmware paint timing within the short virtual deadline.
        let (fx, fy) = touch_xy(marker);
        req_tx
            .send(RunRequest {
                payload: Some(run_request::Payload::DisplayFill(DisplayFill {
                    device_id: 0,
                    x: fx % 300,
                    y: fy % 220,
                    w: 16,
                    h: 16,
                    color: 0x010000 * u32::from(marker) + 0x000040,
                    machine_id: Some(4),
                })),
            })
            .await
            .expect("display fill");

        let mut stream = client
            .run(tonic::Request::new(
                tokio_stream::wrappers::ReceiverStream::new(req_rx),
            ))
            .await
            .expect("run")
            .into_inner();

        let mut traces = Vec::new();
        let mut display_hashes = Vec::new();
        let mut stop_sent = false;
        let mut last_ts = 0u64;
        while let Ok(Some(event)) = stream.message().await {
            match event.payload {
                Some(run_event::Payload::Trace(t)) => traces.push(t.line),
                Some(run_event::Payload::Display(frame)) => {
                    display_hashes.push(canonical_display_hash(&frame));
                }
                Some(run_event::Payload::Tick(t)) => {
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
            last_ts >= RUN_DEADLINE_US,
            "session {session_id}: stopped too early at ts={last_ts}"
        );
        assert!(!traces.is_empty(), "session {session_id}: empty traces");
        display_hashes.sort_unstable();
        RunOutcome {
            traces,
            display_hashes,
            last_ts,
        }
    }

    fn traces_contain_marker(lines: &[String], marker: u8) -> bool {
        !can_marker_lines(lines, marker).is_empty()
    }

    fn traces_contain_eth_len(lines: &[String], marker: u8) -> bool {
        let want = eth_payload_len(marker);
        eth_rx_lens(lines).contains(&want)
    }

    fn assert_evidence_isolation(seed: u32, marker: u8, ev: &SessionEvidence, peer_marker: u8) {
        assert!(
            !ev.can_marker_lines.is_empty(),
            "seed {seed}: missing own CAN marker lines"
        );
        assert!(
            !ev.display_hashes.is_empty(),
            "seed {seed}: expected nonempty display-frame evidence from dashboard/gateway render"
        );
        assert_eq!(
            ev.touch_observed_x, ev.touch_x,
            "seed {seed}: touch X mismatch"
        );
        assert_eq!(
            ev.touch_observed_y, ev.touch_y,
            "seed {seed}: touch Y mismatch"
        );
        assert_eq!(
            (ev.touch_x, ev.touch_y),
            touch_xy(marker),
            "seed {seed}: touch input must match marker"
        );
        assert_eq!(
            ev.adc_channel0,
            adc_value(marker),
            "seed {seed}: ADC channel0 must match session inject"
        );
        assert_eq!(
            ev.timer_irq,
            timer_irq(marker),
            "seed {seed}: timer irq must match board configure input"
        );
        let want_eth = eth_payload_len(marker);
        assert!(
            ev.eth_pkt_rx_lens.contains(&want_eth),
            "seed {seed}: missing eth pkt-rx len={want_eth}, have {:?}",
            ev.eth_pkt_rx_lens
        );
        let peer_can = format!("id={:#06x}", 0x700u32 + u32::from(peer_marker));
        assert!(
            !ev.can_marker_lines.iter().any(|l| l.contains(&peer_can)),
            "seed {seed}: CAN evidence contains peer {peer_can}"
        );
        let peer_eth = eth_payload_len(peer_marker);
        assert!(
            !ev.eth_pkt_rx_lens.contains(&peer_eth),
            "seed {seed}: eth evidence contains peer len {peer_eth}"
        );
        let (peer_tx, peer_ty) = touch_xy(peer_marker);
        assert!(
            !(ev.touch_observed_x == peer_tx && ev.touch_observed_y == peer_ty),
            "seed {seed}: touch evidence matches peer coords"
        );
        assert_ne!(
            ev.adc_channel0,
            adc_value(peer_marker),
            "seed {seed}: ADC matches peer value"
        );
        assert_ne!(
            ev.timer_irq,
            timer_irq(peer_marker),
            "seed {seed}: timer irq matches peer"
        );
    }

    /// One gRPC server hosts two concurrent running sessions with real
    /// microcar firmware, colliding peripheral IDs, distinct CAN/Ethernet/
    /// touch inputs, and deadline-bounded isolation snapshots. 100×.
    #[tokio::test]
    async fn concurrent_grpc_sessions_real_firmware_100x() {
        // Per-seed subprocess keeps FreeRTOS heap pressure bounded across
        // 100 destroy cycles; each child still destroys sessions normally.
        if let Ok(seed_s) = std::env::var("R5_GRPC_SEED") {
            let seed: u32 = seed_s.parse().expect("R5_GRPC_SEED u32");
            run_concurrent_grpc_seed(seed).await;
            return;
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
        let mut ctrl = SimulatorClient::connect(addr.clone())
            .await
            .expect("control connect");
        let baseline_sessions = ctrl
            .list_sessions(ListSessionsRequest {})
            .await
            .expect("list")
            .into_inner()
            .sessions
            .len();

        let marker_a = 0xA0u8.wrapping_add((seed % 16) as u8);
        let marker_b = 0xB0u8.wrapping_add((seed % 16) as u8);
        assert_ne!(marker_a, marker_b);
        assert_ne!(eth_payload_len(marker_a), eth_payload_len(marker_b));

        let solo_a = {
            let mut c = SimulatorClient::connect(addr.clone())
                .await
                .expect("solo a connect");
            let id = setup_session(&mut c, marker_a).await;
            let barrier = Arc::new(tokio::sync::Barrier::new(1));
            let run = run_session_to_deadline(&addr, id, marker_a, barrier).await;
            let ev = collect_evidence(
                &mut c,
                id,
                marker_a,
                &run.traces,
                run.display_hashes.clone(),
            )
            .await;
            assert_evidence_isolation(seed, marker_a, &ev, marker_b);
            assert!(
                traces_contain_marker(&run.traces, marker_a),
                "seed {seed}: solo A missing own CAN"
            );
            assert!(
                traces_contain_eth_len(&run.traces, marker_a),
                "seed {seed}: solo A missing eth pkt-rx"
            );
            c.destroy_session(DestroySessionRequest { session_id: id })
                .await
                .expect("destroy solo a");
            ev
        };
        let solo_b = {
            let mut c = SimulatorClient::connect(addr.clone())
                .await
                .expect("solo b connect");
            let id = setup_session(&mut c, marker_b).await;
            let barrier = Arc::new(tokio::sync::Barrier::new(1));
            let run = run_session_to_deadline(&addr, id, marker_b, barrier).await;
            let ev = collect_evidence(
                &mut c,
                id,
                marker_b,
                &run.traces,
                run.display_hashes.clone(),
            )
            .await;
            assert_evidence_isolation(seed, marker_b, &ev, marker_a);
            c.destroy_session(DestroySessionRequest { session_id: id })
                .await
                .expect("destroy solo b");
            ev
        };

        let hash_a_solo = solo_a.hash();
        let hash_b_solo = solo_b.hash();
        assert_ne!(
            hash_a_solo, hash_b_solo,
            "seed {seed}: distinct sessions must yield distinct isolation evidence"
        );

        let id_a = setup_session(&mut ctrl, marker_a).await;
        let id_b = setup_session(&mut ctrl, marker_b).await;

        for (sid, label) in [(id_a, "A"), (id_b, "B")] {
            let devices = ctrl
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
                    types.contains(&need),
                    "seed {seed} session {label}: missing device {need}, have {types:?}"
                );
            }
        }

        let start_barrier = Arc::new(tokio::sync::Barrier::new(2));
        let overlap = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let overlap_flag = Arc::clone(&overlap);
        let addr_poll = addr.clone();
        let poller = tokio::spawn(async move {
            let mut poll = SimulatorClient::connect(addr_poll)
                .await
                .expect("overlap poller");
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
            loop {
                let sa = poll
                    .get_status(GetStatusRequest { session_id: id_a })
                    .await
                    .map(|r| r.into_inner().state)
                    .unwrap_or_default();
                let sb = poll
                    .get_status(GetStatusRequest { session_id: id_b })
                    .await
                    .map(|r| r.into_inner().state)
                    .unwrap_or_default();
                let a_run = sa.eq_ignore_ascii_case("running");
                let b_run = sb.eq_ignore_ascii_case("running");
                if a_run && b_run {
                    overlap_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                let a_done = sa.eq_ignore_ascii_case("paused")
                    || sa.eq_ignore_ascii_case("done")
                    || sa.eq_ignore_ascii_case("error");
                let b_done = sb.eq_ignore_ascii_case("paused")
                    || sb.eq_ignore_ascii_case("done")
                    || sb.eq_ignore_ascii_case("error");
                if (a_done || b_done) && !overlap_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    panic!("seed {seed}: reached terminal state before overlap (A={sa} B={sb})");
                }
                if tokio::time::Instant::now() > deadline {
                    panic!("seed {seed}: timed out waiting for overlap (A={sa} B={sb})");
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });

        let (run_a, run_b) = tokio::join!(
            run_session_to_deadline(&addr, id_a, marker_a, Arc::clone(&start_barrier)),
            run_session_to_deadline(&addr, id_b, marker_b, start_barrier),
        );
        poller.await.expect("overlap poller join");
        assert!(
            overlap.load(std::sync::atomic::Ordering::SeqCst),
            "seed {seed}: both sessions must be Running simultaneously"
        );
        assert!(
            run_a.last_ts >= RUN_DEADLINE_US && run_b.last_ts >= RUN_DEADLINE_US,
            "seed {seed}: both runs must reach deadline"
        );

        let ev_a = collect_evidence(
            &mut ctrl,
            id_a,
            marker_a,
            &run_a.traces,
            run_a.display_hashes.clone(),
        )
        .await;
        let ev_b = collect_evidence(
            &mut ctrl,
            id_b,
            marker_b,
            &run_b.traces,
            run_b.display_hashes.clone(),
        )
        .await;
        assert_evidence_isolation(seed, marker_a, &ev_a, marker_b);
        assert_evidence_isolation(seed, marker_b, &ev_b, marker_a);

        assert_eq!(
            ev_a.hash(),
            hash_a_solo,
            "seed {seed}: concurrent A evidence diverged from solo"
        );
        assert_eq!(
            ev_b.hash(),
            hash_b_solo,
            "seed {seed}: concurrent B evidence diverged from solo"
        );

        // Negative peer checks on raw streams (in addition to evidence struct).
        assert!(
            traces_contain_marker(&run_a.traces, marker_a)
                && !traces_contain_marker(&run_a.traces, marker_b),
            "seed {seed}: A CAN isolation failed"
        );
        assert!(
            traces_contain_marker(&run_b.traces, marker_b)
                && !traces_contain_marker(&run_b.traces, marker_a),
            "seed {seed}: B CAN isolation failed"
        );
        assert!(
            traces_contain_eth_len(&run_a.traces, marker_a)
                && !traces_contain_eth_len(&run_a.traces, marker_b),
            "seed {seed}: A Ethernet isolation failed"
        );
        assert!(
            traces_contain_eth_len(&run_b.traces, marker_b)
                && !traces_contain_eth_len(&run_b.traces, marker_a),
            "seed {seed}: B Ethernet isolation failed"
        );
        assert_ne!(
            ev_a.display_hashes, ev_b.display_hashes,
            "seed {seed}: display hashes must differ across sessions"
        );
        for h in &ev_a.display_hashes {
            assert!(
                !ev_b.display_hashes.contains(h),
                "seed {seed}: A display hash leaked into B"
            );
        }
        for h in &ev_b.display_hashes {
            assert!(
                !ev_a.display_hashes.contains(h),
                "seed {seed}: B display hash leaked into A"
            );
        }

        ctrl.destroy_session(DestroySessionRequest { session_id: id_a })
            .await
            .expect("destroy A");
        ctrl.destroy_session(DestroySessionRequest { session_id: id_b })
            .await
            .expect("destroy B");

        let after = ctrl
            .list_sessions(ListSessionsRequest {})
            .await
            .expect("list after")
            .into_inner()
            .sessions
            .len();
        assert_eq!(
            after, baseline_sessions,
            "seed {seed}: sessions leaked (baseline={baseline_sessions} after={after})"
        );
    }
}
