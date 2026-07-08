# microcar Vehicle Model

microcar models a **compact passenger EV** as a small network of embedded ECUs
communicating over deterministic CAN-like buses under costar's virtual-time
event loop. The goal is not physical fidelity — it is to reproduce the *shape*
of a real vehicle control network so that every costar product surface (session
isolation, trace correlation, topology, device inspection, breakpoints, replay,
hostile input) is exercised by realistic embedded behavior.

This document is the authoritative description of the vehicle's ECUs, modes, and
signals. Automotive-specific behavior lives in microcar; generic simulation
infrastructure lives in costar.

## ECUs

Each ECU is a costar `Machine` running FreeRTOS (or Zephyr for the dashboard)
firmware on the fiber scheduler. The four core ECUs are the permanent base of
the vehicle; later dogfood lanes add stub ECUs (body control, diagnostics tool,
telematics, charger) only where a lane needs them.

| ECU            | Role                                                                       | Firmware                         |
|----------------|----------------------------------------------------------------------------|----------------------------------|
| **Gateway**    | Vehicle-mode authority, heartbeat monitor, fault aggregator, bus bridge    | `firmware/gateway_ecu`           |
| **Powertrain** | Accelerator/brake processing, torque command, torque limits                | `firmware/powertrain_ecu`        |
| **BMS**        | State of charge, pack voltage/current/temperature, torque limits, faults   | `firmware/bms_ecu`               |
| **Dashboard**  | Speed, warnings, drive mode, user-facing state                             | `firmware/dashboard_ecu` (or Zephyr) |

Responsibilities:

- **Gateway** is the single authority for the vehicle mode. It monitors ECU
  heartbeats, aggregates faults from every ECU, and (in multi-bus topology)
  bridges frames between buses while preventing forwarding loops. It is the
  only ECU allowed to command a mode transition.
- **Powertrain** consumes driver input (accelerator/brake) and the torque limit
  published by the BMS, and emits a torque command. Brake always overrides
  throttle. It clamps its command to the current torque limit and refuses to
  produce positive torque outside `DRIVE`.
- **BMS** publishes pack state (SOC, voltage, current, temperature) and a torque
  limit, and raises pack faults (overtemp, overcurrent, undervoltage). Under a
  critical fault it commands a torque limit of zero.
- **Dashboard** renders user-facing state: current mode, speed, drive mode, and
  active warnings. It is the cockpit-lane display/touch target.

## Vehicle Modes

The Gateway owns the mode state machine. Current modes:

| Mode            | Meaning                                                        | Drive torque |
|-----------------|----------------------------------------------------------------|--------------|
| `OFF`           | Vehicle unpowered; no ECU heartbeats expected                  | blocked      |
| `ACCESSORY`     | Low-power accessories on; propulsion inhibited                 | blocked      |
| `READY`         | Systems booted and healthy; awaiting `DRIVE`                   | blocked      |
| `DRIVE`         | Normal driving; powertrain accepts torque commands            | allowed      |
| `LIMITED_POWER` | Degraded operation (e.g. BMS reduced torque limit)            | reduced      |
| `FAULT`         | A critical fault is active; propulsion disabled                | blocked      |

Later dogfood lanes add:

| Mode           | Lane          | Meaning                                             |
|----------------|---------------|-----------------------------------------------------|
| `CHARGING`     | charging      | Plugged in and charging; drive blocked              |
| `SERVICE`      | diagnostics   | Service/diagnostic session active; drive disabled   |
| `OTA_UPDATE`   | ota           | Firmware update in progress; drive/charge inhibited |
| `TRANSPORT_MODE` | (future)    | Low-power shipping mode                              |

### Mode transition rules (safety invariants)

These rules are enforced by the Gateway and asserted by the dogfood harness:

1. `DRIVE` is entered only from `READY` and only when no critical fault is
   active and at least one powertrain ECU is present and healthy.
2. A critical BMS/pack fault forces `LIMITED_POWER` (reduced limit) or `FAULT`
   (zero limit); positive drive torque must not be commanded in `FAULT`.
3. `CHARGING`, `SERVICE`, and `OTA_UPDATE` all block `DRIVE`; entering them from
   `DRIVE` is rejected.
4. Brake input always overrides throttle regardless of mode.
5. Loss of a required ECU heartbeat drives the vehicle out of `DRIVE`.

## Bus Topology

Phase-1 topology introduced by the `topology` dogfood lane:

```
vcan_drive        vcan_body          vcan_diag
  gateway           gateway            gateway
  powertrain        dashboard          diagnostics_tool
  bms               body_control
```

The Gateway is the only ECU present on more than one bus; it bridges frames
between buses and must forward each frame exactly once, preserving the frame's
trace correlation ID and never re-injecting a frame back onto its origin bus
(loop prevention). Single-bus scenarios (`vcan0` with all four ECUs) remain the
default for the base drive scenarios.

## Signals

Representative CAN-like signals (see `docs/message_protocol.md` for byte
layouts):

- Driver input: accelerator percent, brake pressed
- Powertrain: torque command, actual torque
- BMS: SOC, pack voltage, pack current, pack temperature, torque limit, fault code
- Gateway: vehicle mode, heartbeat, aggregated fault state
- Dashboard: displayed speed, drive mode, warning flags

## Relationship to costar Capabilities

Each vehicle behavior is chosen because it forces a costar capability. See
`docs/costar_microcar_dogfood_plan.md` for the full lane-to-capability mapping;
the short version:

- Concurrent fleet runs → per-session world isolation, no process-global clocks.
- Malformed scenarios → structured validation errors, panic isolation.
- Multi-bus gateway bridging → topology graph, correlation-ID preservation.
- Cockpit display/touch → per-World device ownership, gRPC streaming.
- Seeded bugs → stepping, breakpoints, keyframe replay.
- Diagnostics/telematics/charging/OTA → service workflows, host networking,
  storage/reboot/rollback, and fault-injection robustness.
