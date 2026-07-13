# Message Protocol

The microcar uses a fixed-size CAN-like message protocol. All payloads are packed structs with maximum 8 bytes.

## Message IDs

| ID   | Name                 | Source        | Payload Size |
|------|----------------------|---------------|--------------|
| 0x001| HEARTBEAT            | all ECUs      | 5 bytes      |
| 0x010| VEHICLE_MODE         | gateway       | 2 bytes      |
| 0x020| DRIVER_INPUT         | test harness  | 3 bytes      |
| 0x100| POWERTRAIN_STATUS    | powertrain    | 4 bytes      |
| 0x101| MOTOR_COMMAND        | powertrain    | 2 bytes      |
| 0x102| WHEEL_SPEED          | plant         | 2 bytes      |
| 0x200| BMS_STATUS           | bms           | 7 bytes      |
| 0x201| BMS_LIMITS           | bms           | 2 bytes      |
| 0x202| BMS_FAULT            | bms           | 1 bytes      |
| 0x300| DASHBOARD_STATUS     | dashboard     | 4 bytes      |
| 0x400| WARNING              | any           | 2 bytes      |
| 0x600| DIAG_REQUEST         | diagnostics   | 4 bytes      |
| 0x601| DIAG_RESPONSE        | gateway       | 6 bytes      |
| 0x203| BMS_CHARGE_LIMIT     | bms           | 7 bytes      |
| 0x610| EVSE_EVENT           | evse (6)      | 6 bytes      |
| 0x611| CHARGE_COMMAND       | gateway       | 6 bytes      |
| 0x630| OTA_REQUEST          | ota tool (7)  | 4 bytes      |
| 0x631| OTA_CHUNK            | ota tool (7)  | 8 bytes      |
| 0x632| OTA_FINISH           | ota tool (7)  | 7 bytes      |
| 0x633| OTA_STATUS           | gateway       | 8 bytes      |

## Node IDs

| ID  | Node              |
|-----|-------------------|
| 1   | Gateway ECU       |
| 2   | Powertrain ECU    |
| 3   | BMS ECU           |
| 4   | Dashboard ECU     |
| 5   | Diagnostics tool  |
| 100 | Plant model       |
| 200 | Test harness      |
| 6   | EVSE charger      |
| 7   | OTA tool          |
| 8   | Telematics unit   |

## Payload Formats

### HEARTBEAT (0x001)

```
Offset  Size  Field
0       1     node_id (uint8)
1       4     uptime_ms (uint32, little-endian)
```

### VEHICLE_MODE (0x010)

```
Offset  Size  Field
0       1     mode (uint8: 0=OFF,1=READY,2=DRIVE,3=LIMP,4=FAULT,5=CHARGING,6=SERVICE,7=OTA_UPDATE,8=TRANSPORT_MODE)
1       1     fault_code (uint8)
```

### DRIVER_INPUT (0x020)

```
Offset  Size  Field
0       1     throttle_percent (uint8, 0-100)
1       1     brake_pressed (uint8, 0 or 1)
2       1     gear (uint8, 0=P,1=R,2=N,3=D)
```

### MOTOR_COMMAND (0x101)

```
Offset  Size  Field
0       1     torque_percent (int8, -100..100, negative=regen)
1       1     enable (uint8, 0 or 1)
```

### WHEEL_SPEED (0x102)

```
Offset  Size  Field
0       2     speed_kph_x10 (uint16, little-endian, 120 = 12.0 km/h)
```

### BMS_STATUS (0x200)

```
Offset  Size  Field
0       2     pack_voltage_mv (uint16, little-endian)
2       2     pack_current_ma (int16, little-endian, positive=discharge)
4       2     pack_temp_c_x10 (int16, little-endian, 250 = 25.0°C)
6       1     soc_percent (uint8, 0-100)
```

### BMS_LIMITS (0x201)

```
Offset  Size  Field
0       1     max_torque_percent (uint8, 0-100)
1       1     reason (uint8)
```

### BMS_FAULT (0x202)

```
Offset  Size  Field
0       1     fault_code (uint8: 0=NONE,1=OVERTEMP,2=OVERVOLTAGE,...)
```

### WARNING (0x400)

```
Offset  Size  Field
0       1     source_node (uint8)
1       1     warning_code (uint8)
```

### DIAG_REQUEST (0x600)

```
Offset  Size  Field
0       1     source_node (uint8: 5=diagnostics tool)
1       1     service (uint8: 1=START_SESSION,2=READ_MODE,3=READ_DTCS,4=CLEAR_DTCS,5=LIVE_BMS,6=ACTUATOR_TEST,7=END_SESSION)
2       1     request_id (uint8)
3       1     param (uint8)
```

### DIAG_RESPONSE (0x601)

```
Offset  Size  Field
0       1     source_node (uint8: 1=gateway)
1       1     service (uint8)
2       1     request_id (uint8)
3       1     status (uint8: 0=OK,1=REJECTED,2=UNSUPPORTED,3=STALE)
4       1     value0 (uint8; mode or DTC count depending on service)
5       1     value1 (uint8; first DTC code for READ_DTCS)
```

### BMS_CHARGE_LIMIT (0x203) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 3 BMS)
1       1     max_current_a_x2 (uint8, 0.5 A units)
2       1     soc_percent (uint8)
3       2     temp_c_x10 (int16, little-endian)
5       1     fault (uint8)
6       1     seq (uint8, wrapping)
```

### EVSE_EVENT (0x610) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 6 EVSE)
1       1     event (uint8: 0=UNPLUG,1=PLUG,2=HANDSHAKE_OK,3=STOP)
2       1     request_id (uint8, nonzero)
3       1     offered_current_a_x2 (uint8, 0.5 A units)
4       1     target_soc (uint8, clamped 50..100)
5       1     reserved (uint8)
```

### CHARGE_COMMAND (0x611) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 1 gateway)
1       1     state (uint8: charging FSM state 0..6)
2       1     request_id (uint8)
3       1     current_a_x2 (uint8, 0.5 A units)
4       1     target_soc (uint8)
5       1     reason (uint8, nonzero on rejection)
```

Charging FSM states: 0=DISCONNECTED, 1=PLUG_DETECTED, 2=HANDSHAKE, 3=ACTIVE,
4=LIMITED, 5=COMPLETE, 6=FAULT.

### OTA_REQUEST (0x630) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 7 OTA tool)
1       1     request_id (uint8, nonzero)
2       1     image_id (uint8)
3       1     total_chunks (uint8)
```

### OTA_CHUNK (0x631) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 7 OTA tool)
1       1     request_id (uint8)
2       1     chunk_index (uint8, starts at 0, +1 each, < total_chunks)
3       5     data (5 bytes, test images use 5-byte chunks)
```

### OTA_FINISH (0x632) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 7 OTA tool)
1       1     request_id (uint8)
2       1     total_chunks (uint8, == OTA_REQUEST.total_chunks and observed count)
3       4     crc32 (uint32, little-endian; reflected IEEE poly 0xEDB88320)
```

### OTA_STATUS (0x633) — Stage C

```
Offset  Size  Field
0       1     source_node (uint8, = 1 gateway)
1       1     request_id (uint8)
2       1     state (uint8, OTA slot state)
3       1     status (uint8)
4       1     active_slot (uint8)
5       1     target_slot (uint8)
6       1     reason (uint8)
7       1     seq (uint8)
```

### LIVE_BMS diagnostics selectors (Stage C/D)

`MC_DIAG_LIVE_BMS` (service 5) request `param` selects the reported field:
- `0`: `value0 = soc_percent`, `value1 = temp_c + 40`.
- `1`: `value0..1` = pack voltage in 100 mV, little-endian u16.
- `2`: `value0..1` = pack current in 100 mA, little-endian i16.

A stale (age > 500 ms) or missing snapshot returns status `MC_DIAG_STALE` (3)
with zero values.

> Note: BMS_STATUS (0x200) gains a `seq` byte at offset 7 (8 bytes total) as
> part of Stage D, coordinated with regenerated golden traces; the table above
> still lists the current 7-byte form until that lane lands.

## CAN Frame Encoding

Each message fits within a single CAN frame:

```
[4 bytes id] [1 byte sender] [1 byte len] [1-8 bytes payload]
```

For bus transmission, the C structure `mc_can_frame_t` is used (see `common/include/microcar_can.h`).
