// dashboard_state.h — dashboard state management
//
// Receives vehicle mode, wheel speed, and battery state from the bus
// and maintains a local display state.  Dashboard failure must not
// disable powertrain (S7).

#ifndef DASHBOARD_STATE_H
#define DASHBOARD_STATE_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Maximum number of warnings that can be tracked.
#define DASHBOARD_MAX_WARNINGS 16

/// A single warning displayed on the dashboard.
typedef struct {
    uint8_t warning_code;   // MC_WARN_* constant
    uint8_t active;         // 1 if this warning is currently active
    uint8_t source_node;    // node that originated the warning
} dashboard_warning_t;

/// Dashboard display state.
typedef struct {
    /// Current vehicle mode (OFF/READY/DRIVE/LIMP/FAULT/CHARGING).
    mc_vehicle_mode_t vehicle_mode;

    /// Current wheel speed in 0.1 km/h.
    uint16_t speed_kph_x10;

    /// Battery state of charge (0-100%).
    uint8_t soc_percent;

    /// Battery temperature in 0.1°C.
    int16_t battery_temp_c_x10;

    /// Battery voltage in mV.
    uint16_t battery_voltage_mv;

    /// Active warnings.
    dashboard_warning_t warnings[DASHBOARD_MAX_WARNINGS];
    uint8_t warning_count;

    /// Set when dashboard has received at least one update.
    uint8_t initialized;

    /// Boot flag.
    uint8_t booted;
} dashboard_state_t;

/// Initialise the dashboard state.
void dashboard_state_init(dashboard_state_t *ds);

/// Update vehicle mode from gateway message.
void dashboard_state_set_mode(dashboard_state_t *ds, mc_vehicle_mode_t mode);

/// Update wheel speed from powertrain/plant message.
void dashboard_state_set_speed(dashboard_state_t *ds, uint16_t speed_kph_x10);

/// Update battery state from BMS message.
void dashboard_state_set_battery(dashboard_state_t *ds,
                                 uint8_t soc_percent,
                                 int16_t temp_c_x10,
                                 uint16_t voltage_mv);

/// Add or update a warning on the dashboard.
void dashboard_state_add_warning(dashboard_state_t *ds,
                                 uint8_t warning_code,
                                 uint8_t source_node);

/// Clear a specific warning.
void dashboard_state_clear_warning(dashboard_state_t *ds,
                                   uint8_t warning_code);

/// Clear all warnings from a specific source node.
void dashboard_state_clear_node_warnings(dashboard_state_t *ds,
                                         uint8_t source_node);

/// Get the most severe active warning code (0 = none).
uint8_t dashboard_state_top_warning(const dashboard_state_t *ds);

/// Reboot the dashboard (reset all state).
void dashboard_state_reboot(dashboard_state_t *ds);

#ifdef __cplusplus
}
#endif

#endif // DASHBOARD_STATE_H
