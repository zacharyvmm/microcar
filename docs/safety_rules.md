# Safety Rules

The microcar implements the following safety contract. These rules become scenario assertions and are checked by test scripts.

## Core Safety Contract

| Rule | Description                                          | Verification Method           |
|------|------------------------------------------------------|-------------------------------|
| S1   | If brake is pressed, motor torque must be 0          | Assertion over trace events   |
| S2   | If vehicle mode is FAULT, motor must be disabled     | Assertion over trace events   |
| S3   | If BMS requests LIMP, torque must be capped ≤25% within 300ms | Timed assertion     |
| S4   | If gateway heartbeat is lost, powertrain must disable torque within 250ms | Timed assertion |
| S5   | If BMS heartbeat is lost, gateway must enter FAULT within 300ms | Timed assertion    |
| S6   | If throttle input is invalid, powertrain must ignore it and publish a fault | Event check |
| S7   | Dashboard failure must not disable powertrain by itself | Scenario test         |
| S8   | Critical BMS fault must shut down drive regardless of throttle | Scenario test |

## Timing Constants

| Constant                          | Value | Description                               |
|-----------------------------------|-------|-------------------------------------------|
| Heartbeat interval                | 100ms | All ECUs send heartbeat every 100ms       |
| Heartbeat timeout                 | 300ms | Missing heartbeats after 300ms → timeout  |
| LIMP torque response              | 300ms | Torque must be capped within 300ms of LIMP|
| Gateway timeout for powertrain    | 250ms | Powertrain disables torque after 250ms    |

## BMS Temperature Thresholds

| Temperature | BMS State         | Gateway Response |
|-------------|-------------------|------------------|
| ≤ 60°C      | BMS_OK            | Normal operation |
| 60-75°C     | BMS_WARN_HOT      | Warning to dashboard |
| 75-90°C     | BMS_LIMP_REQUEST  | LIMP mode, torque ≤ 25% |
| > 90°C      | BMS_CRITICAL_FAULT| FAULT mode, motor disabled |

## Fault Response Table

| Condition                            | Expected Gateway Action         |
|--------------------------------------|---------------------------------|
| all required ECUs online             | enter VEHICLE_READY             |
| driver throttle active and no faults | enter VEHICLE_DRIVE             |
| BMS overtemperature warning          | enter VEHICLE_LIMP              |
| BMS critical fault                   | enter VEHICLE_FAULT             |
| BMS heartbeat lost                   | enter VEHICLE_FAULT             |
| powertrain heartbeat lost            | enter VEHICLE_FAULT             |
| dashboard heartbeat lost             | warning only, not drive-inhibit |
| invalid driver input                 | publish warning/fault           |
| charger plugged while driving        | enter VEHICLE_FAULT             |
