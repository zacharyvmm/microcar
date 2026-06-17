// microcar_safety.h — safety contract constants
//
// Defines timing requirements and thresholds that enforcement logic
// and test assertions should reference.

#ifndef MICROCAR_SAFETY_H
#define MICROCAR_SAFETY_H

// ── Timing Requirements (milliseconds) ───────────────────────────────────

#define MC_SAFETY_LIMP_RESPONSE_MS         300   // S3: torque capped within 300ms of LIMP
#define MC_SAFETY_GATEWAY_TIMEOUT_MS       250   // S4: powertrain disables torque within 250ms
#define MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS 300   // S5: gateway enters FAULT within 300ms
#define MC_SAFETY_HEARTBEAT_INTERVAL_MS    100   // All ECUs send heartbeat every 100ms

// ── BMS Temperature Thresholds (°C) ─────────────────────────────────────

#define MC_BMS_TEMP_OK             60    // Below this: BMS_OK
#define MC_BMS_TEMP_WARN           60    // Above this: BMS_WARN_HOT
#define MC_BMS_TEMP_LIMP           75    // Above this: BMS_LIMP_REQUEST
#define MC_BMS_TEMP_CRITICAL       90    // Above this: BMS_CRITICAL_FAULT

// ── Torque Limits ───────────────────────────────────────────────────────

#define MC_TORQUE_LIMP_MAX_PERCENT 25    // Max torque in LIMP mode
#define MC_TORQUE_FAULT_MAX_PERCENT 0    // Max torque in FAULT mode

// ── Safety Rules (documented for assertions) ────────────────────────────

// S1: If brake is pressed, motor torque must be 0.
// S2: If vehicle mode is FAULT, motor must be disabled.
// S3: If BMS requests LIMP, torque must be capped within 300ms.
// S4: If gateway heartbeat is lost, powertrain must disable torque within 250ms.
// S5: If BMS heartbeat is lost, gateway must enter FAULT within 300ms.
// S6: If throttle input is invalid, powertrain must ignore it and publish a fault.
// S7: Dashboard failure must not disable powertrain by itself.
// S8: Critical BMS fault must shut down drive regardless of throttle.

#endif // MICROCAR_SAFETY_H
