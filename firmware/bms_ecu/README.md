# BMS ECU

Battery Management System — monitors battery state, enforces temperature limits.

## Responsibilities

- Read virtual battery temperature/current/voltage/SOC
- Publish battery status
- Publish torque/current limit
- Request limp mode on high temperature
- Request fault mode on critical temperature
- Publish heartbeat

## States

- `BMS_OK` — normal operation
- `BMS_WARN_HOT` — temperature > 60°C
- `BMS_LIMP_REQUEST` — temperature > 75°C
- `BMS_CRITICAL_FAULT` — temperature > 90°C

## Temperature Thresholds

| Temperature | BMS State            | Gateway Action     |
|-------------|----------------------|---------------------|
| ≤ 60°C      | BMS_OK               | Normal              |
| 60-75°C     | BMS_WARN_HOT         | Warning to dashboard|
| 75-90°C     | BMS_LIMP_REQUEST     | LIMP mode           |
| > 90°C      | BMS_CRITICAL_FAULT   | FAULT mode          |
