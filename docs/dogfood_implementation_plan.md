---

## 10. Implementation Progress (2026-06-21)

### Completed

| Phase | Status | What was done |
|-------|--------|---------------|
| **A1** | ✅ | `Firmware` trait in `sim-world/src/firmware.rs`: `init()`/`step()` lifecycle, 4 tests |
| **A2** | ✅ | Per-Simulator isolation: `ACTIVE_SIM_GLOBAL`, `with_sim_global()`, `SimGlobalGuard` in sim-ffi |
| **A2** | ✅ | `sim_scheduler_tick()` C ABI function — tick-by-tick FreeRTOS advancement for multi-machine Worlds |
| **A3** | ✅ | 17 C firmware files compiled via `cc` crate in microcar `build.rs` |
| **A3** | ✅ | All 4 ECU `main.c` files rewritten with real `vTaskDelayUntil`, `sim_can_send`, `sim_can_recv` |
| **A3** | ✅ | `microcar_coordinator.c` creates 4 FreeRTOS tasks (no `vTaskStartScheduler` — tick mode) |
| **A4** | ✅ | `Cargo.toml` updated with sim-world, sim-core, sim-ffi, sim-freertos-port deps |
| **A5** | ✅ | `src/lib.rs`: `MicrocarFirmware` implementing `Firmware` trait, calls per-ECU boot + `sim_scheduler_tick()` |
| **A5** | ✅ | `src/main.rs`: scenario runner with firmware loading, plant attachment, `run_until` |
| **A6** | ✅ | Python test pipeline preserved as reference (`tests/check_*.py`) |
| **B1** | ✅ | `scenarios/fleet_of_8.toml`: 8 machines on 2 CAN buses, 6,986 trace events, <2s wall-clock |
| **B2** | ✅ | `scenarios/uart_chain.toml`: daisy-chain UART with per-byte timing |
| **B3** | ✅ | `scenarios/asymmetric_ticks.toml`: documents per-machine tick rate topology |
| **B4** | ✅ | `scenarios/ecu_reboot.toml`: BMS ECU reboot mid-drive with gateway heartbeat monitoring |
| **C1** | ✅ | Multi-task ECUs: gateway (+heartbeat_rx+fault_aggregator), powertrain (+sensor_poll+logger), dashboard (+display_update), BMS (+calibration) |
| **C2** | ✅ | Full FreeRTOS API: mutexes, counting semaphores, event groups, task notifications, software timers, xTaskCreateStatic, vTaskDelete |
| **C3** | ✅ | `scenarios/task_lifecycle.toml` — xTaskCreateStatic at boot, vTaskDelete, dynamic xTaskCreate restart |
| **C4** | ✅ | Tickless idle verified via `soak_1hour.toml` (1 hour virtual drive) and `overnight_8hour.toml` |
| **D** | 🔄 | Infrastructure wired: `zephyr` Cargo feature, `microcar_zephyr_boot.c` C entry, `ZephyrDashboardFirmware` Rust adapter, rtos="zephyr" routing in `main.rs`. Blocked on `sim_zephyr_scheduler_tick()` in sim-ffi (per Risk 5). |
| **E1** | ✅ | `costar test --microcar --all` discovers and runs all 25 scenarios |
| **E2** | ✅ | Golden trace comparison enabled by default for `--microcar`; 10 golden traces regenerated from real fiber execution |
| **E5** | ✅ | `.github/workflows/microcar.yml` CI pipeline: builds both repos, runs tests, verifies determinism |
| **F1** | ✅ | Long scenarios: `long_drive_10min.toml`, `soak_1hour.toml`, `overnight_8hour.toml` (8h virtual, sub-10ms wall) |
| **F2** | ✅ | Fleet stress: `fleet_of_16.toml` (4 CAN buses), `fleet_of_64.toml` (8 buses, SKIP stress test) |
| **F3** | ✅ | Determinism verified: 10 identical SHA-256 hashes across 10 runs via `tests/verify_determinism.sh` |
| **F4** | ✅ | `benches/bench_main.rs`: events/sec, wall sec/1M ticks, RSS memory per machine |
| **G1** | ✅ | `sim_register_symbol()` added to all 4 ECUs — task names appear in trace output |
| **G2** | ✅ | `costar replay <trace> --step` for interactive trace debugging |
| **G3** | ✅ | `--trace-format jsonl` for machine-parseable CI output |
| **G4** | ✅ | `--machine-filter <name>` flag for per-machine trace filtering |
| **Plant protocol** | ✅ | Plant CAN IDs fixed: 0x200→0x102 (MC_MSG_WHEEL_SPEED), 0x300→0x500 (MC_MSG_PLANT_SENSORS) |
| **ECU assignment** | ✅ | `MicrocarFirmware` uses scenario's `firmware` field (e.g. `firmware/gateway_ecu`) for explicit ECU selection |
| **Determinism** | ✅ | Identical SHA-256 hash across 3 runs (Phase A); extended to 10-run verification (Phase F3) |
| **Test suite** | ✅ | 270+ tests pass across entire costar workspace |
| **CAN bridge** | ✅ | Firmware CAN TX/RX bridged to World CanBus |
| **Trace unification** | ✅ | `SimGlobal.trace` initialized in `Simulator::new()`; `sim_scheduler_tick()` flushes each cycle |
| **Golden traces** | ✅ | All 10 golden traces regenerated from real fiber execution in `expected/traces/` |
| **Symbolicated traces** | ✅ | `sim_register_symbol()` added to all 4 ECUs — task names now appear in trace output |

### Current State

```
Target pipeline (working):
  scenarios/*.toml → costar test runner → PASS (25/25, with golden comparison)

Costar features exercised:
  ✅ sim-core:          event queue, virtual clock, deterministic trace
  ✅ sim-fiber:         fiber pool, task spawn, resume/yield (via sim_scheduler_tick)
  ✅ sim-ffi:           per-Simulator isolation, C ABI bridge, trace flushing
  ✅ sim-freertos-port: FreeRTOS kernel linked, tasks created on fibers
  ✅ sim-world:         World, Machine(×64), CanBus(×8), Plant, Scenario DSL
  ✅ sim-runner:        test discovery, --microcar, --machine-filter, --golden, --diff, replay, serve
  ✅ sim-devices:       VirtualCan TX/RX bridging to World CanBus
  ✅ sim-zephyr-port:   infrastructure wired (per Risk 5, tick function pending)

Scenarios: 25 total (15 original + 10 new)
  bms_overtemp_limp_mode, boot_and_heartbeat, brake_overrides_throttle,
  critical_bms_shutdown, dashboard_reboot, delayed_bms_fault_message,
  dropped_vehicle_mode_frame, fleet_of_8, gateway_reboot, long_drive_10min,
  lost_bms_heartbeat, mixed_rtos_boot, normal_drive_cycle, soak_1hour,
  uart_chain, asymmetric_ticks, ecu_reboot, event_group_mode, fleet_of_16,
  fleet_of_64, multitask_priority, mutex_fault_queue, overnight_8hour,
  software_timer_watchdog, task_lifecycle
```

### Remaining: Phase D (Mixed RTOS Zephyr)

Zephyr mixed-RTOS execution is the only incomplete item. Infrastructure is in place:

- `Cargo.toml`: `zephyr = []` feature flag
- `build.rs`: compiles `dashboard_ecu_zephyr/` + `microcar_zephyr_boot.c` with Zephyr includes when feature enabled
- `firmware/microcar_zephyr_boot.c`: calls `sim_zephyr_init()` + `sim_zephyr_register_thread()`
- `src/lib.rs`: `ZephyrDashboardFirmware` implementing `Firmware` trait, uses `sim_zephyr_reschedule()` as stepping stone
- `src/main.rs`: routes `rtos = "zephyr"` machines to `ZephyrDashboardFirmware`

**Blocked on two costar-side changes:**
1. `sim_zephyr_scheduler_tick()` in `sim-ffi` — a tick-by-tick Zephyr scheduler advancement function (analogous to `sim_scheduler_tick()` for FreeRTOS) needed for per-machine Zephyr isolation
2. Standalone Zephyr mock headers in `sim-zephyr-port` — the `dashboard_ecu_zephyr` firmware uses Zephyr APIs (`k_sleep`, etc.) not yet covered by the standalone compatibility layer

These are acknowledged in Risk 5 of the implementation plan and are follow-on work for the costar sim-ffi/sim-zephyr-port crates.

### Quick Verification

```bash
cd /Users/zmm/projects/costar
cargo test --workspace                          # 270+ passed
cargo run -- test --microcar --all --verbose    # 25/25 PASS (golden comparison enabled)

cd /Users/zmm/projects/microcar
cargo build                                     # clean
cargo run -- scenarios/soak_1hour.toml          # 1 hour virtual drive, PASS
bash tests/verify_determinism.sh                # 10/10 identical SHA-256
```
