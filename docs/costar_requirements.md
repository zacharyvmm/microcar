# costar Requirements

Features needed from the `costar` simulator for the microcar demo.

## Immediately Needed (for MVP)

- [x] `World` — multi-machine event loop with shared virtual clock
- [x] `Machine` — per-machine simulator instances with event queues
- [x] `Link` — deterministic FIFO/UART links between machines
- [x] `Scenario` — TOML-based scenario loading and validation
- [x] `costar run --scenario` — scenario execution from CLI
- [x] `costar test` — headless CI scenario test runner
- [x] Broadcast bus (CAN-like) — all-to-all with sender exclusion
  - Implemented as `CanBus` in sim-world with true broadcast semantics, fault injection, and deterministic ordering (Phase 3)
- [x] Plant/environment callbacks — external model integration
  - Implemented as `EnvironmentModel` trait in sim-world with `MicrocarPlant` in microcar-plant crate (Phase 4)
- [x] `[[bus]]` and `[[bus.node]]` in scenario format
- [x] `[[fault]]` in scenario format — timed fault injection (parsed, no-op for now)
- [x] `[[input]]` in scenario format — timed driver inputs (parsed, no-op for now)
- [x] `[[expect.event]]` in scenario format — expected event checks (parsed, validation only)
- [x] `[plant]` section in scenario format (parsed, informational)
- [x] Machine `firmware` field — named firmware target per machine
- [x] `rtos` field in machine config
- [x] Trace prefixes with virtual time and machine name
- [x] `costar run --scenario` from external repos (path dependency) — works via absolute/relative path

## Needed Soon

- [ ] Fault injection: drop/delay/corrupt bus frames
- [ ] Node stop/reboot support
- [x] Plant/environment callback integration
- [ ] Generic virtual ADC or sensor input
- [ ] Scenario assertion support (`[[assert]]`)
- [ ] Deterministic random seed support
- [ ] `costar test --all` scenario discovery

## Needed Later

- [ ] `sim-can` crate — generic CAN-like broadcast bus
  - Implemented as `CanBus` in `sim-world/canbus.rs` (Phase 3)
- [ ] `sim-assert` crate — generic scenario assertion engine
- [ ] CAN arbitration and bus load timing
- [ ] Virtual CAN controller device
- [ ] Virtual EEPROM/flash
- [ ] Watchdog device
- [ ] Zephyr external app integration for dashboard ECU
- [ ] Process isolation for unsafe C firmware
- [ ] Trace viewer or timeline export
- [ ] Multiple CAN buses with gateway routing

## Phase 2: Minimal costar runner integration (2026-06-17)

### Completed
- Extended `Scenario` struct to accept all microcar fields: `duration_ms`, `bus`, `bus.node`, `plant`, `input`, `fault`, `expect.event`, `assert`
- Added `firmware` and `rtos` optional fields to `MachineDef`
- Bus topology handled: `[[bus]]` + `[[bus.node]]` expands to N*(N-1) FIFO links
- All new fields validated: bus name uniqueness, machine references, fault target format, expect.event machine references
- 69 sim-world tests pass, full workspace (252 tests) passes
- Clippy clean with `-D warnings`

### What works
- `costar run --scenario <path>` loads and parses microcar scenario TOML files
- Bus topology correctly creates N*(N-1) links between attached nodes
- All microcar scenario files parse successfully (11 scenarios)
- Scenario with no `[expect]` section runs to completion (e.g., `long_drive_10min.toml`)
- `costar test` can be used to run scenarios (existing test runner unchanged)

### Gaps / Known Issues
- Golden trace files (`expected/traces/*.trace`) don't exist yet — scenarios with `[expect]` fail validation
  - Fix: create expected trace directories and golden files, or make trace path validation non-fatal
- `[[input]]`, `[[fault]]`, `[[expect.event]]` are parsed but not acted on during simulation
  - These require firmware integration (plant model callbacks, fault injection hooks, event assertion engine)
- `[[assert]]` section defined but unused
- Bus is modeled as N*(N-1) point-to-point links, not true broadcast — adequate for MVP but not accurate for CAN arbitration
- Machines run empty (no firmware tasks) — actual ECU firmware needs to be loaded and executed

## Notes

- The `sim-world` crate's current `Link` model is point-to-point FIFO, not broadcast
- Scenario format now supports buses, plants, inputs, faults, and expected events
- The `costar test` subcommand uses simple `[[machine]]`/`[[link]]` scenarios in `tests/scenarios/`
- Plant integration uses `EnvironmentModel` trait in `sim-world` with `MicrocarPlant` in `microcar-plant` crate
- `[[input]]` entries are queued as driver inputs on the plant model and applied at their scheduled virtual times

## Phase 4: Plant model integration (2026-06-17)

### Completed
- Added `EnvironmentModel` trait to `sim-world` crate (`crates/sim-world/src/plant.rs`)
- Added plant support to `World`: `set_plant()`, `step_plant()`, `queue_plant_input()`
- World run loop calls `plant.step()` at the configured tick rate (from `[plant].tick_ms`)
- Plant ticks are included in `next_global_event_time()` for deterministic scheduling
- `Scenario::attach_plant_to()` — attaches a plant model and queues all `[[input]]` entries
- `Scenario::check_trace()` — public trace comparison logic for external callers
- `run_scenario` and `run_scenario_test` create `MicrocarPlant` when `[plant].type = "microcar"`
- World run loop continues stepping plant even when machines are idle
- `duration_ms` from scenario bounds the simulation via `run_until()`
- `MicrocarPlant` implements `EnvironmentModel`: publishes wheel speed (CAN ID 0x200) and BMS sensor readings (CAN ID 0x300) each tick
- Golden trace files generated for `normal_drive_cycle` and `boot_and_heartbeat`

### CAN protocol
| ID     | Name                | Publisher | Format                              |
|--------|---------------------|-----------|-------------------------------------|
| 0x0001 | Heartbeat           | ECUs      | [machine_id, timestamp_msb, ...]    |
| 0x0200 | MC_MSG_WHEEL_SPEED  | plant     | [speed_hi, speed_lo] (u16 BE)       |
| 0x0300 | MC_MSG_BMS_STATUS   | plant     | [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo] |
| 0x0100 | MC_MSG_MOTOR_COMMAND| powertrain| [torque_i8, 0, 0, 0, 0]            |

### What works
- `costar run --scenario ../microcar/scenarios/normal_drive_cycle.toml` runs plant model
- Plant publishes wheel speed and BMS readings each tick
- Driver inputs from `[[input]]` applied at scheduled virtual times
- Speed increases with throttle, battery SOC decreases under load
- Golden trace comparison passes for all updated scenarios
- 13 plant tests pass, 82 sim-world tests pass
