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
- [ ] Plant/environment callbacks — external model integration
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
- [ ] Plant/environment callback integration
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
- Plant integration likely needs a trait (`EnvironmentModel`) in `sim-world` with microcar providing the concrete implementation
