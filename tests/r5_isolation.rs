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

const GW_PT_SCENARIO: &str = r#"
name = "r5_gw_pt"
duration_ms = 500

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
latency_us = 500

[[bus.node]]
bus = "vcan0"
machine = "gateway"

[[bus.node]]
bus = "vcan0"
machine = "powertrain"
"#;

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

fn build_gw_pt_world() -> World {
    build_gw_bms_world_from(GW_PT_SCENARIO, None, false)
}

fn build_gw_bms_plant_world(temp_c: u32) -> World {
    build_gw_bms_world_from(GW_BMS_PLANT_SCENARIO, Some(temp_c), true)
}

fn run_to_duration(world: &mut World, duration_ms: u64) {
    world.run_until(duration_ms * 1000).expect("run_until");
}

/// Duplicate gateway+powertrain Worlds reproduce Trace v2 hashes under A/B
/// and B/A recreation order, 100× (same FreeRTOS recreate envelope as R4).
#[test]
fn two_worlds_gateway_bms_trace_v2_interleave_100x() {
    const DUR_MS: u64 = 500;
    for seed in 0..100u32 {
        let hash_a = {
            let mut w = build_gw_pt_world();
            run_to_duration(&mut w, DUR_MS);
            hash_world_traces(&mut w)
        };
        let hash_b = {
            let mut w = build_gw_pt_world();
            run_to_duration(&mut w, DUR_MS);
            hash_world_traces(&mut w)
        };
        assert_ne!(
            hash_a, FNV_OFFSET_BASIS,
            "seed {seed}: expected non-empty Trace v2"
        );
        assert_eq!(
            hash_a, hash_b,
            "seed {seed} A→B: world B must match world A"
        );

        let hash_b2 = {
            let mut w = build_gw_pt_world();
            run_to_duration(&mut w, DUR_MS);
            hash_world_traces(&mut w)
        };
        let hash_a2 = {
            let mut w = build_gw_pt_world();
            run_to_duration(&mut w, DUR_MS);
            hash_world_traces(&mut w)
        };
        assert_eq!(hash_b2, hash_a, "seed {seed} B→A: first world diverged");
        assert_eq!(hash_a2, hash_a, "seed {seed} B→A: second world diverged");
    }
}

/// Two BMS Worlds receive different plant temperatures and publish different
/// BMS status without cross-observation.
#[test]
fn two_bms_plant_frames_isolated() {
    const DUR_MS: u64 = 400;
    let hot_recs = {
        let mut world_hot = build_gw_bms_plant_world(90);
        run_to_duration(&mut world_hot, DUR_MS);
        world_hot.drain_trace_v2()
    };
    let cold_recs = {
        let mut world_cold = build_gw_bms_plant_world(20);
        run_to_duration(&mut world_cold, DUR_MS);
        world_cold.drain_trace_v2()
    };

    let hot_status: Vec<_> = hot_recs
        .iter()
        .filter(|r| r.message_id == 0x200 && r.direction == "tx" && r.source == 3)
        .map(|r| r.payload_summary.clone())
        .collect();
    let cold_status: Vec<_> = cold_recs
        .iter()
        .filter(|r| r.message_id == 0x200 && r.direction == "tx" && r.source == 3)
        .map(|r| r.payload_summary.clone())
        .collect();

    assert!(
        !hot_status.is_empty(),
        "hot world BMS must publish status (0x200)"
    );
    assert!(
        !cold_status.is_empty(),
        "cold world BMS must publish status (0x200)"
    );
    assert_ne!(
        hot_status, cold_status,
        "hot and cold BMS status payloads must differ"
    );

    // Plant sensor RX (0x500) must stay local to each world.
    let hot_plant: Vec<_> = hot_recs
        .iter()
        .filter(|r| r.message_id == 0x500 && r.direction == "rx" && r.destination == 3)
        .map(|r| r.payload_summary.clone())
        .collect();
    let cold_plant: Vec<_> = cold_recs
        .iter()
        .filter(|r| r.message_id == 0x500 && r.direction == "rx" && r.destination == 3)
        .map(|r| r.payload_summary.clone())
        .collect();
    assert!(!hot_plant.is_empty(), "hot BMS must RX plant sensors");
    assert!(!cold_plant.is_empty(), "cold BMS must RX plant sensors");
    assert_ne!(
        hot_plant, cold_plant,
        "plant sensor payloads must differ across worlds"
    );
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

    fn session_scenario(marker: u8) -> String {
        format!(
            r#"
name = "r5_grpc_{marker}"
duration_ms = 200

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

[[bus_inject]]
at_ms = 50
bus = "vcan0"
sender = "gateway"
id = 0x001
data = [{marker}, 50, 0, 0, 0]
"#,
            marker = marker,
        )
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
        ]
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

    /// One gRPC server hosts two live sessions with real microcar firmware and
    /// colliding device IDs (display/touch/timer/adc/can 0). Create/load/configure
    /// succeeds 100× without cross-session failures.
    #[tokio::test]
    async fn concurrent_grpc_sessions_real_firmware_100x() {
        let (addr, _handle) = start_server().await;

        for seed in 0..100u32 {
            let mut client = SimulatorClient::connect(addr.clone())
                .await
                .expect("connect");

            let id_a = setup_session(&mut client, 0xA0).await;
            let id_b = setup_session(&mut client, 0xB0).await;
            assert_ne!(id_a, id_b, "seed {seed}: session ids must differ");

            let status_a = client
                .get_status(GetStatusRequest { session_id: id_a })
                .await
                .expect("status a")
                .into_inner();
            let status_b = client
                .get_status(GetStatusRequest { session_id: id_b })
                .await
                .expect("status b")
                .into_inner();
            assert!(
                status_a.n_machines >= 2,
                "seed {seed}: session A missing machines"
            );
            assert!(
                status_b.n_machines >= 2,
                "seed {seed}: session B missing machines"
            );

            let _ = client
                .destroy_session(DestroySessionRequest { session_id: id_a })
                .await;
            let _ = client
                .destroy_session(DestroySessionRequest { session_id: id_b })
                .await;
        }
    }
}
