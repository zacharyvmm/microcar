# microcar

A **compact passenger-EV embedded-system benchmark** for
[costar](https://github.com/zacharyvmm/costar). microcar simulates a
simplified electric-vehicle ECU network — safety behavior, diagnostics,
cockpit devices, telematics, charging, OTA, and fault scenarios — under
deterministic virtual time. It is the canonical dogfood project for costar:
each vehicle subsystem exists to force a specific costar capability, not to
model high-fidelity car physics.

> microcar is **not** a go-kart, toy car, RC car, or physics simulator. It is
> a deterministic embedded-systems benchmark whose job is to exercise the
> simulator surfaces a product needs: gRPC sessions, concurrent runs, device
> inspection, display/touch streaming, trace correlation, topology graphing,
> breakpoints, replay, and hostile input.

The core ECUs model a compact EV's control network, connected by deterministic
CAN-like buses:

- **Gateway ECU** — vehicle mode authority, heartbeat monitor, fault aggregator, bus bridge
- **Powertrain ECU** — accelerator/brake processing, torque command, torque limits
- **BMS ECU** — state of charge, voltage, current, temperature, torque limits, pack faults
- **Dashboard ECU** — speed, warnings, drive mode, user-facing state (FreeRTOS or Zephyr)

Vehicle modes: `OFF`, `ACCESSORY`, `READY`, `DRIVE`, `LIMITED_POWER`, `FAULT`
(with `CHARGING`, `SERVICE`, `OTA_UPDATE`, and `TRANSPORT_MODE` added by later
dogfood lanes).

It dogfoods costar features including:

- multi-machine simulation and multi-bus topology
- FreeRTOS tasks, queues, timers, and event groups
- Zephyr RTOS threading (k_thread_create, k_sleep)
- mixed-RTOS scenarios (FreeRTOS + Zephyr on the same bus)
- deterministic virtual time
- virtual sensors and actuators
- CAN-like message passing
- fault injection and malformed-scenario robustness
- golden traces and trace-hash determinism
- safety assertions and invariant checking

See `docs/costar_microcar_dogfood_plan.md` for the canonical roadmap that links
each dogfood lane (simfarm, toml_zoo, topology, cockpit, debug_gym, diagnostics,
telematics, charging, OTA) to the costar capability it forces.

## Quick Start

```bash
# Prerequisite: clone costar alongside microcar
git clone https://github.com/zacharyvmm/costar.git ../costar

# Build (includes compiling C firmware via build.rs)
cargo build

# Run state machine tests (pure logic, no simulation runtime)
cargo test -p microcar-state-tests

# Run a scenario through the Rust simulation binary
cargo run -- scenarios/normal_drive_cycle.toml

# Run all scenarios through the Python assertion engine
./tests/run_all.sh

# Verify deterministic output (10 runs, SHA-256 comparison)
bash tests/verify_determinism.sh
```

## Repository Layout

```
├── src/               — binary entry point (main.rs) and firmware bridge (lib.rs)
├── build.rs           — compiles C firmware into the Rust binary
├── plant/             — vehicle plant model crate (speed, battery, sensors)
├── state_tests/       — pure state machine logic tests (no simulation runtime)
├── common/            — shared C protocol definitions
│   ├── include/       — microcar_protocol.h, microcar_can.h
│   └── src/           — microcar_protocol.c
├── firmware/          — per-ECU C application code
│   ├── gateway_ecu/
│   ├── powertrain_ecu/
│   ├── bms_ecu/
│   ├── dashboard_ecu/        — FreeRTOS dashboard
│   ├── dashboard_ecu_zephyr/ — Zephyr dashboard (feature-gated)
│   ├── zephyr_mock/          — Zephyr API mock for host compilation
│   ├── microcar_coordinator.c
│   └── microcar_zephyr_boot.c
├── scenarios/         — TOML scenario definitions (18+ scenarios)
├── expected/traces/   — golden traces for validation
├── tests/             — test scripts (run_all.sh, check_assertions.py, etc.)
├── examples/          — example programs (boot_test, diag, show_trace)
├── benches/           — performance benchmarks
├── docs/              — architecture, protocol, and requirements documentation
└── .github/workflows/ — CI pipeline (build, test, determinism check)
```

## Running a Scenario

```bash
# Rust binary (compiles and links C firmware via build.rs)
cargo run -- scenarios/normal_drive_cycle.toml

# Python assertion engine (Python-only simulation, no costar runtime)
python3 tests/check_assertions.py scenarios/normal_drive_cycle.toml

# With golden trace comparison
python3 tests/check_assertions.py scenarios/normal_drive_cycle.toml \
    expected/traces/normal_drive_cycle.trace
```

## Running Tests

```bash
# State machine logic tests
cargo test -p microcar-state-tests

# Run all scenarios through assertion engine
./tests/run_all.sh

# Single scenario
./tests/run_one.sh scenarios/bms_overtemp_limp_mode.toml

# Deterministic verification (10 runs, checks SHA-256)
bash tests/verify_determinism.sh
```

## Zephyr Support

The dashboard ECU can run on Zephyr RTOS instead of FreeRTOS.
Build with the `zephyr` feature:

```bash
cargo build --features zephyr
cargo run --features zephyr -- scenarios/mixed_rtos_boot.toml
```

See `docs/architecture.md` for details on mixed-RTOS scenarios,
Zephyr configuration, and standalone vs. full-kernel compilation.

## Relationship to costar

`microcar` is a dogfood demo for [costar](https://github.com/zacharyvmm/costar).
It exercises costar's multi-machine simulation, deterministic virtual time,
FreeRTOS integration, Zephyr integration, and bus communication.
The two repos are designed to be cloned side-by-side:

```
projects/
├── costar/    — simulation infrastructure (World, fibers, CAN bus, RTOS ports)
└── microcar/  — application (ECU firmware, plant model, scenarios, traces)
```

See `docs/costar_requirements.md` for features needed from costar.
