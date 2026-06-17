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
