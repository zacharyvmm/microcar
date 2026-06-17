// bms_limits.c — BMS torque limit computation implementation

#include "bms_limits.h"
#include <string.h>

void bms_limits_init(bms_limits_t *bl)
{
    memset(bl, 0, sizeof(*bl));
    bl->max_torque_percent = 255; // unlimited
    bl->reason             = 0;
}

uint8_t bms_limits_compute(bms_limits_t *bl, mc_bms_state_t bms_state)
{
    switch (bms_state) {
    case BMS_OK:
        bl->max_torque_percent = 255; // unlimited
        bl->reason             = 0;
        break;

    case BMS_WARN_HOT:
        // Warning but no torque limit yet
        bl->max_torque_percent = 255;
        bl->reason             = 1; // warning_reason
        break;

    case BMS_LIMP_REQUEST:
        bl->max_torque_percent = MC_TORQUE_LIMP_MAX_PERCENT; // 25%
        bl->reason             = 2; // limp_reason
        break;

    case BMS_CRITICAL_FAULT:
        bl->max_torque_percent = MC_TORQUE_FAULT_MAX_PERCENT; // 0%
        bl->reason             = 3; // fault_reason
        break;

    default:
        bl->max_torque_percent = 255;
        bl->reason             = 0;
        break;
    }

    return bl->max_torque_percent;
}

uint8_t bms_limits_limp_max(void)
{
    return MC_TORQUE_LIMP_MAX_PERCENT;
}
