# costar Microcar Demo

A deterministic distributed embedded simulation of a tiny electric go-kart.

This demo contains several simulated ECUs connected by a deterministic CAN-like bus:

- Gateway ECU — central supervisor, heartbeat monitor, fault manager
- Powertrain ECU — throttle/brake processing, motor command, torque limiting
- Battery Management ECU — battery state, temperature/current limits, fault reporting
- Dashboard ECU — display state, speed, warnings, heartbeat

It dogfoods costar features including:

- multi-machine simulation
- FreeRTOS tasks, queues, timers, and event groups
- deterministic virtual time
- virtual sensors and actuators
- CAN-like message passing
- fault injection
- golden traces
- safety assertions

## Quick Start

```bash
cd /home/zmm/projects/microcar
cargo build
cargo test
./tests/run_all.sh
```

## Repository Layout

- `common/` — shared protocol definitions (message IDs, payload structs)
- `plant/` — vehicle plant model (Rust crate)
- `firmware/` — per-ECU application code
- `boards/` — board peripheral configuration
- `scenarios/` — TOML scenario definitions
- `expected/` — golden traces and expected final states
- `tests/` — test scripts and assertion checkers
- `tools/` — trace analysis utilities
- `docs/` — architecture, protocol, and requirements documentation

## Relationship to costar

`microcar` is a dogfood demo for [costar](https://github.com/zmm/costar). It exercises costar's multi-machine simulation, deterministic virtual time, FreeRTOS integration, and bus communication. See `docs/costar_requirements.md` for features needed from costar.
