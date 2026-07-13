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
3       1     status (uint8: 0=OK,1=REJECTED,2=UNSUPPORTED)
4       1     value0 (uint8; mode or DTC count depending on service)
5       1     value1 (uint8; first DTC code for READ_DTCS)
```

## CAN Frame Encoding

Each message fits within a single CAN frame:

```
[4 bytes id] [1 byte sender] [1 byte len] [1-8 bytes payload]
```

For bus transmission, the C structure `mc_can_frame_t` is used (see `common/include/microcar_can.h`).
