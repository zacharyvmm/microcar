// bms_limits.h — BMS torque limit computation
//
// Based on BMS state, computes the maximum allowable torque.
// Published via MC_MSG_BMS_LIMITS.

#ifndef BMS_LIMITS_H
#define BMS_LIMITS_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// BMS limits state.
typedef struct {
    /// Current maximum torque percent allowed (0-100, 255=unlimited).
    uint8_t max_torque_percent;

    /// Reason code for the limit (maps to BMS state).
    uint8_t reason;
} bms_limits_t;

/// Initialise BMS limits to unlimited.
void bms_limits_init(bms_limits_t *bl);

/// Compute torque limits based on BMS state.
/// Returns the max torque percent.
uint8_t bms_limits_compute(bms_limits_t *bl, mc_bms_state_t bms_state);

/// Get the max torque for LIMP mode (from safety header).
uint8_t bms_limits_limp_max(void);

#ifdef __cplusplus
}
#endif

#endif // BMS_LIMITS_H
