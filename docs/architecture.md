# Architecture

The microcar demo models a small electric go-kart with four simulated ECUs connected by a deterministic CAN-like bus.

## ECU Topology

```
driver inputs
    ↓
gateway ECU (supervisor)
    ↓
powertrain ECU → motor command → simulated vehicle plant
    ↑                                ↓
BMS ECU ← battery/temp/current sensors ← plant state
    ↓
dashboard ECU (display)
```

## Bus Architecture

All ECUs are connected to a single broadcast bus (`vcan0`). Frames are:

- CAN-like with sender node ID, message ID, 8-byte payload
- Delivered to all attached nodes except the sender
- Deterministic with configurable latency (default 500 µs)
- Ordered by virtual time, message priority, and sequence number

See `docs/message_protocol.md` for the complete message catalog.

## Plant Model

The `plant/` crate models:

- Vehicle speed (acceleration from torque, deceleration from drag)
- Battery SOC, voltage, temperature
- Sensor readings published to the bus

The plant is intentionally simple and deterministic — no real physics, all fixed-point math.

## Simulation Flow

1. Scenario file is loaded (TOML)
2. World is created with all machines, links, plant model
3. Scenario injects driver inputs and faults at specific virtual times
4. ECUs boot, exchange heartbeats, enter READY state
5. Driver throttle is injected, powertrain commands torque
6. Plant updates speed/battery, publishes sensor readings
7. Faults are injected, safety logic responds
8. Trace is compared against expected output
9. Results: PASS or FAIL with diffs

## Key Design Decisions

- ECUs communicate ONLY through the simulated bus — no direct function calls
- All time is virtual — the sim runs as fast as possible, no wall-clock sleeps
- Rust owns fiber lifecycle and scheduling; C firmware runs on corosensei fibers
- Plant model is in `microcar` (application-specific), bus infrastructure in `costar` (generic)

## Mixed-RTOS Support

As of Phase 7, costar supports mixed-RTOS scenarios where different machines
run different RTOS backends:

| ECU         | Default RTOS | Alternative |
|-------------|-------------|-------------|
| Gateway     | FreeRTOS    | —           |
| Powertrain  | FreeRTOS    | —           |
| BMS         | FreeRTOS    | —           |
| Dashboard   | FreeRTOS    | Zephyr      |

### Scenario Configuration

The `rtos` field in `[[machine]]` sections selects the RTOS backend:

```toml
[[machine]]
id = 4
name = "dashboard"
rtos = "zephyr"   # or "freertos" (default)
```

### Machine struct

`sim-world::Machine` stores the `rtos` field (type: `String`).  The
`Scenario::build_world()` method reads the `rtos` field from `MachineDef`
and passes it to `Machine::with_rtos()`.

### Zephyr Dashboard ECU

The Zephyr dashboard ECU (`firmware/dashboard_ecu_zephyr/`) reuses the
same business logic (`dashboard_state.c`, `warning_display.c`) as the
FreeRTOS version.  The main entry point (`main.c`) uses Zephyr threading
APIs (`k_thread_create`, `k_sleep`) and is compiled via costar's
`sim-zephyr-port` cc crate.

**Standalone compilation** (standalone Zephyr test, no real Zephyr kernel):
```bash
cd costar
cargo run -- --rtos zephyr --golden
```

**Full Zephyr kernel compilation** (requires Zephyr v4.1.0 source tree):
```bash
cd costar
ZEPHYR_BASE=/path/to/zephyr-workspace/zephyr \
ZEPHYR_APP_SOURCES=/path/to/microcar/firmware/dashboard_ecu_zephyr/src/main.c \
ZEPHYR_EXTRA_SOURCES="/path/to/microcar/firmware/dashboard_ecu/src/dashboard_state.c \
                     /path/to/microcar/firmware/dashboard_ecu/src/warning_display.c" \
ZEPHYR_APP_INCLUDES="/path/to/microcar/common/include:/path/to/microcar/firmware/dashboard_ecu_zephyr/src:/path/to/microcar/firmware/dashboard_ecu/src" \
cargo build --features zephyr_real
```

### Mixed-RTOS Scenario Test

`scenarios/mixed_rtos_boot.toml` runs 3 FreeRTOS ECUs and 1 Zephyr
dashboard on the same vcan0 bus.  The scenario is validated via
`tests/check_assertions.py` which checks:

- Dashboard receives vehicle mode, speed, and battery state from the bus
- Dashboard publishes heartbeats at 100ms intervals
- Gateway detects dashboard as "node_online" on first heartbeat
- All traces are deterministic (golden trace comparison against
  `expected/traces/mixed_rtos_boot.trace`)

```bash
cd microcar
python3 tests/check_assertions.py scenarios/mixed_rtos_boot.toml
```
