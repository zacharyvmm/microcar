// safety_rules.h — safety rule enforcement for powertrain
//
// Implements the safety rules S1-S8 (as applicable to powertrain).
// Each rule is a single function for testability.

#ifndef SAFETY_RULES_H
#define SAFETY_RULES_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ── Safety Rule Functions ────────────────────────────────────────────────

/// S1: If brake is pressed, motor torque must be 0.
/// Returns 1 if brake overrides (torque should be 0), 0 otherwise.
uint8_t safety_brake_overrides_throttle(uint8_t brake_pressed);

/// S2: If vehicle mode is FAULT, motor must be disabled.
/// Returns 1 if motor should be disabled, 0 otherwise.
uint8_t safety_fault_disables_motor(mc_vehicle_mode_t mode);

/// S3: If vehicle mode is LIMP, torque must be capped at MC_TORQUE_LIMP_MAX_PERCENT.
/// Returns the effective max torque percent (255 = no cap).
uint8_t safety_limp_torque_cap(mc_vehicle_mode_t mode);

/// S6: If throttle input is invalid (>100%), powertrain must ignore it.
/// Returns 1 if throttle is invalid, 0 otherwise.
uint8_t safety_throttle_invalid(uint8_t throttle_percent);

/// Apply all safety rules to a requested torque.
/// Returns the safe output torque.
int8_t safety_clamp_torque(int8_t requested_torque,
                           mc_vehicle_mode_t mode,
                           uint8_t brake_pressed,
                           uint8_t bms_max_torque);

#ifdef __cplusplus
}
#endif

#endif // SAFETY_RULES_H
