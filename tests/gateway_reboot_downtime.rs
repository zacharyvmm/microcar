//! R4: gateway reboot downtime proves real firmware recovery.
//!
//! Extends the TOML fixture assertions with flash persistence, RTOS/config
//! identity, volatile CAN queue reset, and 100× repeatability.

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

[[fault]]
at_ms = 1000
target = "machine.gateway"
type = "reboot"
downtime_ms = 5
"#;

const GATEWAY_BOARD: &str = r#"
[peripherals]
can0 = { device = "can", id = 0 }
flash0 = { device = "flash", id = 0 }
"#;

const FLASH_MARKER: &[u8] = &[0x4D, 0x43, 0x52, 0x34, 0xB3, 0x01, 0x02, 0x03];

fn build_world() -> World {
    let scenario = Scenario::from_str(SCENARIO).expect("parse scenario");
    let mut world = scenario.build_world().expect("build world");
    world.enable_owned_device_banks();

    let board = BoardConfig::from_str(GATEWAY_BOARD).expect("board");
    world
        .configure_machine_board(1, board)
        .expect("configure gateway board");

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
}

fn write_flash_marker(world: &mut World) {
    world
        .with_machine_devices(1, || {
            let ok = sim_devices::with_flash_mut(0, |flash| {
                assert!(flash.erase_page(0), "erase page 0");
                assert!(
                    flash.write_page(0, 0, FLASH_MARKER),
                    "write flash marker"
                );
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
    assert!(
        traces.iter().any(|l| l.contains("microcar_boot_gateway")),
        "factory must re-run gateway boot after reconstruction"
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
    assert_eq!(tx_len, 0, "volatile CAN TX queue must be empty after reboot");
    assert_eq!(rx_len, 0, "volatile CAN RX queue must be empty after reboot");
}

fn run_once() {
    let mut world = build_world();
    write_flash_marker(&mut world);
    world.run_until(2_000_000).expect("run_until");
    let traces = world.drain_all_traces();
    assert_reboot_invariants(&mut world, &traces);
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
    // Keep the dogfood TOML path green through the product binary wiring.
    let scenario = Scenario::from_file("dogfood/b3_gateway_reboot_downtime.toml")
        .expect("load b3 fixture");
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
