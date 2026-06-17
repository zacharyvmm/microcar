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
- [x] `[[fault]]` in scenario format — timed fault injection
  - Implemented in World with fault scheduling, force_temperature on plant battery, stop_heartbeat, reboot, drop_frame, delay_frame on buses (Phase 5)
- [x] `[[input]]` in scenario format — timed driver inputs
- [x] `[[expect.event]]` in scenario format — expected event checks
  - Validated via check_trace.py with Python-based ECU state machine simulation (Phase 5)
- [x] `[plant]` section in scenario format
- [x] Machine `firmware` field — named firmware target per machine
- [x] `rtos` field in machine config
- [x] Trace prefixes with virtual time and machine name
- [x] `costar run --scenario` from external repos (path dependency)

## Needed Soon

- [x] Fault injection: drop/delay/corrupt bus frames (Phase 5)
- [x] Node stop/reboot support (Phase 5)
- [x] Plant/environment callback integration (Phase 4)
- [x] Scenario assertion support (Phase 5)
- [ ] ECU firmware main.c files compiled into costar runtime (Phase 6+)
- [ ] Generic virtual ADC or sensor input
- [ ] `costar test --all` scenario discovery

## Phase 5: Fault injection and safety scenarios (2026-06-17)

### Completed
- Added `apply_fault()` method to `EnvironmentModel` trait
- World supports fault scheduling: `schedule_fault()` with timed delivery
- Fault types: `force_temperature` (plant.battery), `stop_heartbeat` (machine), `reboot` (machine), `drop_frame` (bus), `delay_frame` (bus)
- `Scenario::run_with_plant()` — supports plant factory for creating plant models
- Duration-bound simulations via `duration_ms` → `run_until()`
- CanBus already supported `drop_frame()` and `delay_frame()` for bus-targeted faults
- Firmware main.c files for all 4 ECUs (gateway, powertrain, BMS, dashboard)
- Comprehensive Python-based ECU state machine simulation in `check_trace.py`
  - GatewayState: heartbeat monitoring, vehicle mode transitions (OFF→READY→DRIVE→LIMP→FAULT), fault aggregation
  - PowertrainState: torque computation, safety enforcement (S1-S4), gateway timeout detection
  - BmsState: temperature threshold checks, fault/limit publication
  - DashboardState: display updates, warning severity management
  - EcuHeartbeats: periodic heartbeat generation with stop/reboot fault support
- `check_trace.py` validates `[[expect.event]]` assertions against simulated trace
- Golden trace files generated for all 11 deterministic scenarios
- `tests/run_all.sh` runs all scenarios and reports PASS/FAIL
- All 11 scenarios pass
- All costar tests pass (266 tests)

### Architecture note
The firmware main.c files implement complete ECU logic integrating state machines with the CAN bus for heartbeat receiving, mode broadcasting, fault aggregation, torque computation with safety enforcement, sensor reading with threshold checks, and warning display updates. The Rust host runtime (costar) will compile and link these C files when the firmware integration phase begins. For now, the Python-based ECU simulation in `check_trace.py` validates the behavioral specification.

## Phase 4: Plant model integration (2026-06-17)

### Completed
- Added `EnvironmentModel` trait to `sim-world` crate
- World run loop calls `plant.step()` at configured tick rate
- `Scenario::attach_plant_to()` attaches plant model and queues inputs
- `MicrocarPlant` publishes wheel speed and BMS sensor readings each tick
- Golden trace files generated for `normal_drive_cycle` and `boot_and_heartbeat`

## Phase 2-3: Bus topology and scenario format (2026-06-17)

### Completed
- Scenario struct extended with all microcar fields
- Bus topology: `[[bus]]` + `[[bus.node]]` → `CanBus` instances with broadcast
- CanBus fault injection: drop_frame, delay_frame, corrupt_byte
- All fields validated (bus name uniqueness, machine references, fault targets)
