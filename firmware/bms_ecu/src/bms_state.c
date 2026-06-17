// bms_state.c — BMS state machine implementation

#include "bms_state.h"
#include <string.h>

void bms_state_init(bms_state_t *bs)
{
    memset(bs, 0, sizeof(*bs));
    bs->state      = BMS_OK;
    bs->fault_code = MC_BMS_FAULT_NONE;
}

mc_bms_state_t bms_state_update(bms_state_t *bs,
                                int16_t temp_c_x10,
                                uint16_t voltage_mv,
                                int16_t current_ma,
                                uint8_t soc_percent)
{
    bs->temp_c_x10  = temp_c_x10;
    bs->voltage_mv  = voltage_mv;
    bs->current_ma  = current_ma;
    bs->soc_percent = soc_percent;

    bs->state = bms_state_from_temp(temp_c_x10);

    // Set fault code based on state
    bs->fault_code = bms_state_fault_code(bs);

    return bs->state;
}

mc_bms_state_t bms_state_from_temp(int16_t temp_c_x10)
{
    // Use the shared helper from protocol.c
    return mc_bms_determine_state(temp_c_x10);
}

uint8_t bms_state_is_fault(const bms_state_t *bs)
{
    return (bs->state == BMS_CRITICAL_FAULT) ? 1 : 0;
}

uint8_t bms_state_is_limp(const bms_state_t *bs)
{
    return (bs->state == BMS_LIMP_REQUEST) ? 1 : 0;
}

uint8_t bms_state_fault_code(const bms_state_t *bs)
{
    switch (bs->state) {
    case BMS_CRITICAL_FAULT:
        return MC_BMS_FAULT_OVERTEMP;
    case BMS_LIMP_REQUEST:
        return MC_BMS_FAULT_NONE; // limp is not a fault, just a limit
    case BMS_WARN_HOT:
        return MC_BMS_FAULT_NONE;
    case BMS_OK:
    default:
        return MC_BMS_FAULT_NONE;
    }
}
