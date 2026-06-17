// warning_display.c — dashboard warning display logic implementation

#include "warning_display.h"
#include <string.h>

void warning_display_init(warning_display_t *wd)
{
    memset(wd, 0, sizeof(*wd));
    wd->displayed_warning = MC_WARN_NONE;
    wd->severity          = 0;
}

uint8_t warning_display_update(warning_display_t *wd,
                               uint8_t warning_code,
                               uint8_t severity)
{
    // Higher severity always wins; equal severity keeps current
    if (severity > wd->severity) {
        wd->displayed_warning = warning_code;
        wd->severity          = severity;
    }
    return wd->displayed_warning;
}

void warning_display_clear(warning_display_t *wd)
{
    wd->displayed_warning = MC_WARN_NONE;
    wd->severity          = 0;
}

uint8_t warning_display_severity_for(uint8_t warning_code)
{
    switch (warning_code) {
    case MC_WARN_CRITICAL_BMS_FAULT:
        return 3;
    case MC_WARN_BMS_OVERTEMP:
    case MC_WARN_POWERTRAIN_OFFLINE:
    case MC_WARN_GATEWAY_RESTARTED:
        return 2;
    case MC_WARN_BMS_OFFLINE:
    case MC_WARN_DASHBOARD_OFFLINE:
    case MC_WARN_INVALID_THROTTLE:
    case MC_WARN_CHARGER_PLUGGED:
        return 1;
    case MC_WARN_NONE:
    default:
        return 0;
    }
}
