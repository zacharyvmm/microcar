// gateway_state.h — gateway ECU vehicle mode state machine
//
// The gateway is the central coordinator. It tracks all ECU heartbeats,
// determines the vehicle's operating mode, and aggregates faults.

#ifndef GATEWAY_STATE_H
#define GATEWAY_STATE_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Gateway internal state.
typedef struct {
    /// Current vehicle mode (OFF/READY/DRIVE/LIMP/FAULT/CHARGING).
    mc_vehicle_mode_t mode;

    /// Set when all expected nodes are online.
    uint8_t all_nodes_online;

    /// Set when BMS has reported a critical fault.
    uint8_t bms_fault_active;

    /// Set when BMS requests limp mode (overtemp warning level).
    uint8_t bms_limp_requested;

    /// Number of active faults aggregated across all ECUs.
    uint8_t active_fault_count;

    /// Boot flag: set after initialisation completes.
    uint8_t booted;
} gateway_state_t;

/// Initialise gateway state to OFF.
void gateway_state_init(gateway_state_t *gs);

/// Update gateway state based on inputs:
/// - all_nodes_online: 1 if all required ECUs are communicating
/// - bms_fault_active: 1 if BMS critical fault is active
/// - bms_limp_requested: 1 if BMS requests limp mode
/// - fault_count: number of active faults
///
/// Returns the new vehicle mode.
mc_vehicle_mode_t gateway_state_update(gateway_state_t *gs,
                                       uint8_t all_nodes_online,
                                       uint8_t bms_fault_active,
                                       uint8_t bms_limp_requested,
                                       uint8_t fault_count);

/// Transition from READY to DRIVE (called when driver input received).
void gateway_state_enter_drive(gateway_state_t *gs);

/// Force a gateway reboot (reset to OFF).
void gateway_state_reboot(gateway_state_t *gs);

/// Get current mode as a string for tracing.
const char *gateway_mode_string(mc_vehicle_mode_t mode);

#ifdef __cplusplus
}
#endif

#endif // GATEWAY_STATE_H
