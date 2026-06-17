// dashboard_state.c — dashboard state management implementation

#include "dashboard_state.h"
#include <string.h>

void dashboard_state_init(dashboard_state_t *ds)
{
    memset(ds, 0, sizeof(*ds));
    ds->booted = 1;
}

void dashboard_state_set_mode(dashboard_state_t *ds, mc_vehicle_mode_t mode)
{
    ds->vehicle_mode = mode;
    ds->initialized  = 1;
}

void dashboard_state_set_speed(dashboard_state_t *ds, uint16_t speed_kph_x10)
{
    ds->speed_kph_x10 = speed_kph_x10;
    ds->initialized   = 1;
}

void dashboard_state_set_battery(dashboard_state_t *ds,
                                 uint8_t soc_percent,
                                 int16_t temp_c_x10,
                                 uint16_t voltage_mv)
{
    ds->soc_percent        = soc_percent;
    ds->battery_temp_c_x10 = temp_c_x10;
    ds->battery_voltage_mv = voltage_mv;
    ds->initialized        = 1;
}

void dashboard_state_add_warning(dashboard_state_t *ds,
                                 uint8_t warning_code,
                                 uint8_t source_node)
{
    // Check if this warning already exists
    for (uint8_t i = 0; i < ds->warning_count; i++) {
        if (ds->warnings[i].warning_code == warning_code &&
            ds->warnings[i].source_node  == source_node) {
            ds->warnings[i].active = 1;
            return;
        }
    }

    // Add new warning
    if (ds->warning_count >= DASHBOARD_MAX_WARNINGS) {
        return; // warning table full
    }

    ds->warnings[ds->warning_count].warning_code = warning_code;
    ds->warnings[ds->warning_count].source_node  = source_node;
    ds->warnings[ds->warning_count].active       = 1;
    ds->warning_count++;
}

void dashboard_state_clear_warning(dashboard_state_t *ds,
                                   uint8_t warning_code)
{
    for (uint8_t i = 0; i < ds->warning_count; i++) {
        if (ds->warnings[i].warning_code == warning_code) {
            ds->warnings[i].active = 0;
        }
    }
}

void dashboard_state_clear_node_warnings(dashboard_state_t *ds,
                                         uint8_t source_node)
{
    for (uint8_t i = 0; i < ds->warning_count; i++) {
        if (ds->warnings[i].source_node == source_node) {
            ds->warnings[i].active = 0;
        }
    }
}

uint8_t dashboard_state_top_warning(const dashboard_state_t *ds)
{
    uint8_t top_code     = MC_WARN_NONE;
    uint8_t top_severity = 0;

    for (uint8_t i = 0; i < ds->warning_count; i++) {
        if (!ds->warnings[i].active) {
            continue;
        }
        uint8_t sev = 0;
        switch (ds->warnings[i].warning_code) {
        case MC_WARN_CRITICAL_BMS_FAULT:
            sev = 3; // critical
            break;
        case MC_WARN_BMS_OVERTEMP:
        case MC_WARN_POWERTRAIN_OFFLINE:
        case MC_WARN_GATEWAY_RESTARTED:
            sev = 2; // warning
            break;
        case MC_WARN_BMS_OFFLINE:
        case MC_WARN_DASHBOARD_OFFLINE:
        case MC_WARN_INVALID_THROTTLE:
        case MC_WARN_CHARGER_PLUGGED:
            sev = 1; // info
            break;
        default:
            sev = 0;
            break;
        }
        if (sev > top_severity) {
            top_severity = sev;
            top_code     = ds->warnings[i].warning_code;
        }
    }

    return top_code;
}

void dashboard_state_reboot(dashboard_state_t *ds)
{
    memset(ds, 0, sizeof(*ds));
    ds->booted = 0;
}
