# Powertrain ECU

Primary FreeRTOS stress node — throttle/brake processing, motor command, torque limiting.

## Responsibilities

- Receive driver input
- Receive vehicle mode from gateway
- Receive BMS current/torque limits
- Compute motor torque command
- Publish powertrain status and heartbeat
- Enforce local safety rules
- Shut down torque on timeout/fault

## States

- `PT_DISABLED` — motor disabled
- `PT_READY` — enabled, waiting for drive command
- `PT_DRIVE` — active torque control
- `PT_LIMP` — torque capped (≤25%)
- `PT_FAULT` — motor disabled, fault condition

## Safety Rules (local)

| Condition                   | Behavior                       |
|-----------------------------|--------------------------------|
| brake and throttle together | brake wins, torque = 0         |
| vehicle mode is LIMP        | torque capped at 25%           |
| vehicle mode is FAULT       | torque = 0, motor disabled     |
| BMS torque limit received   | torque capped                  |
| gateway command timeout     | torque = 0                     |
| invalid throttle value      | torque = 0, fault published    |
| watchdog task starved       | fault published                |
