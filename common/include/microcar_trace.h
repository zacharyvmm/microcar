// microcar_trace.h — trace event naming conventions
//
// All microcar trace events follow the pattern:
//   machine:<name> event:<type> [key=value ...]
//
// This header defines the standard event type strings to ensure
// consistency across ECUs and test scripts.

#ifndef MICROCAR_TRACE_H
#define MICROCAR_TRACE_H

// ── Standard Event Names ─────────────────────────────────────────────────

#define MC_TRACE_BOOT               "boot"
#define MC_TRACE_HEARTBEAT          "heartbeat"
#define MC_TRACE_NODE_ONLINE        "node_online"
#define MC_TRACE_NODE_LOST          "node_lost"
#define MC_TRACE_VEHICLE_MODE       "vehicle_mode"
#define MC_TRACE_DRIVER_INPUT       "driver_input"
#define MC_TRACE_MOTOR_COMMAND      "motor_command"
#define MC_TRACE_TORQUE_LIMITED     "torque_limited"
#define MC_TRACE_WHEEL_SPEED        "wheel_speed"
#define MC_TRACE_BATTERY_STATUS     "battery_status"
#define MC_TRACE_BATTERY_TEMP       "battery_temp"
#define MC_TRACE_BMS_FAULT          "bms_fault"
#define MC_TRACE_DASHBOARD_UPDATE   "dashboard_update"
#define MC_TRACE_WARNING            "warning"
#define MC_TRACE_FAULT_INJECT       "fault_inject"
#define MC_TRACE_SCENARIO_DONE      "scenario_done"
#define MC_TRACE_ECU_REBOOT         "ecu_reboot"
#define MC_TRACE_GATEWAY_TIMEOUT    "gateway_timeout"

// ── Vehicle Mode Names ───────────────────────────────────────────────────

#define MC_MODE_OFF       "OFF"
#define MC_MODE_READY     "READY"
#define MC_MODE_DRIVE     "DRIVE"
#define MC_MODE_LIMP      "LIMP"
#define MC_MODE_FAULT     "FAULT"
#define MC_MODE_CHARGING  "CHARGING"

#endif // MICROCAR_TRACE_H
