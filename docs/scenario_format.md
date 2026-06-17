# Scenario Format

Scenarios are TOML files describing a multi-machine simulation. See `scenarios/` for examples.

## Top-Level Fields

| Field       | Type   | Required | Description                          |
|-------------|--------|----------|--------------------------------------|
| `name`      | string | yes      | Human-readable scenario name         |
| `duration_ms`| uint  | no       | Maximum virtual time (wall-clock)    |

## Machine Definition

```toml
[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
rtos = "freertos"
```

| Field      | Type   | Required | Description                           |
|------------|--------|----------|---------------------------------------|
| `id`       | uint   | yes      | Unique machine ID in scenario         |
| `name`     | string | yes      | Human-readable name                   |
| `firmware` | string | no       | Path to firmware directory            |
| `rtos`     | string | no       | RTOS: "freertos" or "zephyr"         |

## Bus Definition

```toml
[[bus]]
name = "vcan0"
type = "can"
latency_us = 500

[[bus.node]]
bus = "vcan0"
machine = "gateway"
```

| Field       | Type   | Required | Description                        |
|-------------|--------|----------|------------------------------------|
| `name`      | string | yes      | Bus name                           |
| `type`      | string | yes      | "can" (future: "lin", "flexray")  |
| `latency_us`| uint   | no       | Per-hop latency in microseconds    |

## Plant

```toml
[plant]
type = "microcar"
tick_ms = 10
```

## Inputs

```toml
[[input]]
at_ms = 100
type = "driver_input"
throttle_percent = 50
brake_pressed = false
```

## Faults

```toml
[[fault]]
at_ms = 1200
target = "plant.battery"
type = "force_temperature"
value_c = 82
```

Fault types:
- `force_temperature` — set battery temperature
- `stop_heartbeat` — stop a machine's heartbeat
- `reboot` — reboot a machine
- `drop_frame` — drop a specific CAN frame
- `delay_frame` — delay a specific CAN frame by N ms

## Expected Events

```toml
[[expect.event]]
before_ms = 1500
machine = "gateway"
event = "vehicle_mode"
value = "LIMP"
```

| Field       | Type   | Required | Description                          |
|-------------|--------|----------|--------------------------------------|
| `before_ms` | uint   | yes      | Event must occur before this time    |
| `machine`   | string | yes      | Machine that should emit the event   |
| `event`     | string | yes      | Expected event type                  |
| `value`     | string | no       | Expected value (string match)        |
| `max_percent`| uint  | no       | Numeric upper bound                  |

## Assertions (future)

```toml
[[assert]]
name = "limp mode caps torque"
always_when = "vehicle_mode == LIMP"
condition = "motor_torque_percent <= 25"
```

## Expected Output

```toml
[expect]
trace = "expected/traces/bms_overtemp_limp_mode.trace"
```
