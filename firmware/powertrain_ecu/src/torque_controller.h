// torque_controller.h — powertrain torque computation
//
// Computes motor torque command from driver input, vehicle mode,
// and BMS limits. Safety rules are enforced separately.

#ifndef TORQUE_CONTROLLER_H
#define TORQUE_CONTROLLER_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Torque controller state.
typedef struct {
    /// Last received driver throttle (0-100).
    uint8_t throttle_percent;

    /// Last received brake state (0 or 1).
    uint8_t brake_pressed;

    /// Last received gear position.
    uint8_t gear;

    /// Current vehicle mode (from gateway).
    mc_vehicle_mode_t vehicle_mode;

    /// BMS torque limit percent (255 = no limit).
    uint8_t bms_torque_limit;

    /// Computed output torque (-100..100).
    int8_t output_torque;

    /// Motor enable flag (1 = enabled).
    uint8_t motor_enable;
} torque_controller_t;

/// Initialise the torque controller.
void torque_controller_init(torque_controller_t *tc);

/// Set the driver input for this tick.
void torque_controller_set_input(torque_controller_t *tc,
                                 uint8_t throttle, uint8_t brake, uint8_t gear);

/// Set the current vehicle mode (received from gateway).
void torque_controller_set_mode(torque_controller_t *tc,
                                mc_vehicle_mode_t mode);

/// Set the BMS torque limit (255 = no limit).
void torque_controller_set_bms_limit(torque_controller_t *tc,
                                     uint8_t max_torque);

/// Compute the output torque based on current state.
/// Applies all safety rules:
///   - brake → torque=0
///   - FAULT mode → torque=0, motor disabled
///   - LIMP mode → torque capped at MC_TORQUE_LIMP_MAX_PERCENT
///   - BMS limit → further caps torque
/// Returns the computed torque value.
int8_t torque_controller_compute(torque_controller_t *tc);

/// Returns 1 if throttle input is invalid (>100%).
uint8_t torque_controller_throttle_invalid(const torque_controller_t *tc);

#ifdef __cplusplus
}
#endif

#endif // TORQUE_CONTROLLER_H
