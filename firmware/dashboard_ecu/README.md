# Dashboard ECU

Display interface — shows speed, battery, warnings, vehicle mode.

## Responsibilities

- Receive vehicle mode from gateway
- Receive wheel speed from plant
- Receive battery state from BMS
- Receive warnings from gateway and BMS
- Emit trace-friendly display updates
- Publish heartbeat

## State

- speed_kph: current speed
- battery_percent: SOC (0-100%)
- vehicle_mode: OFF/READY/DRIVE/LIMP/FAULT
- limp_warning: LIMP mode active
- fault_warning: FAULT mode active
- bms_hot_warning: BMS overtemp active

## Planned: Zephyr migration

Later, the dashboard ECU will be converted to a Zephyr-based node to dogfood mixed-RTOS support. The safety-critical ECUs (gateway, powertrain, BMS) remain FreeRTOS.
