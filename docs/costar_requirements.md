# costar Requirements

Features needed from the `costar` simulator for the microcar demo.

## Immediately Needed (for MVP)

- [x] `World` — multi-machine event loop with shared virtual clock
- [x] `Machine` — per-machine simulator instances with event queues
- [x] `Link` — deterministic FIFO/UART links between machines
- [x] `Scenario` — TOML-based scenario loading and validation
- [x] `costar run --scenario` — scenario execution from CLI
- [x] `costar test` — headless CI scenario test runner
- [ ] Broadcast bus (CAN-like) — all-to-all with sender exclusion
- [ ] Plant/environment callbacks — external model integration
- [ ] `[[bus]]` and `[[bus.node]]` in scenario format
- [ ] `[[fault]]` in scenario format — timed fault injection
- [ ] `[[input]]` in scenario format — timed driver inputs
- [ ] `[[expect.event]]` in scenario format — expected event checks
- [ ] `[plant]` section in scenario format
- [ ] Machine `firmware` field — named firmware target per machine
- [ ] `rtos` field in machine config
- [ ] Trace prefixes with virtual time and machine name
- [ ] `costar run --scenario` from external repos (path dependency)

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
- [ ] `sim-assert` crate — generic scenario assertion engine
- [ ] CAN arbitration and bus load timing
- [ ] Virtual CAN controller device
- [ ] Virtual EEPROM/flash
- [ ] Watchdog device
- [ ] Zephyr external app integration for dashboard ECU
- [ ] Process isolation for unsafe C firmware
- [ ] Trace viewer or timeline export
- [ ] Multiple CAN buses with gateway routing

## Notes

- The `sim-world` crate's current `Link` model is point-to-point FIFO, not broadcast
- Scenario format needs to grow from current `[[machine]]`/`[[link]]`/`[[inject]]` to support buses, plants, inputs, faults, and expected events
- The `costar test` subcommand exists but uses simple `[[machine]]`/`[[link]]` scenarios in `tests/scenarios/`
- Plant integration likely needs a trait (`EnvironmentModel`) in `sim-world` with microcar providing the concrete implementation
