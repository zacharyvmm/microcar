# Gateway ECU

Central supervisor, heartbeat monitor, and fault manager.

## Responsibilities

- Receive heartbeats from all ECUs
- Mark nodes online/offline
- Aggregate faults
- Broadcast vehicle mode
- Route or rebroadcast important status
- Enforce global safety state
- Notify dashboard of warnings

## States

- `VEHICLE_OFF` — all systems off
- `VEHICLE_READY` — all required ECUs online, no driver activity
- `VEHICLE_DRIVE` — active driving, no faults
- `VEHICLE_LIMP` — degraded operation (BMS overtemperature)
- `VEHICLE_FAULT` — critical fault, motor disabled
- `VEHICLE_CHARGING` — charger plugged

## Key Behavior

| Condition                            | Action                         |
|--------------------------------------|--------------------------------|
| all required ECUs online             | enter VEHICLE_READY            |
| driver throttle active, no faults    | enter VEHICLE_DRIVE            |
| BMS overtemperature warning          | enter VEHICLE_LIMP             |
| BMS critical fault                   | enter VEHICLE_FAULT            |
| BMS heartbeat lost                   | enter VEHICLE_FAULT            |
| powertrain heartbeat lost            | enter VEHICLE_FAULT            |
| dashboard heartbeat lost             | warning only, not drive-inhibit|
