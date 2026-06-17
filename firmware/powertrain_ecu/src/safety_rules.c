// safety_rules.c — safety rule enforcement implementation

#include "safety_rules.h"

uint8_t safety_brake_overrides_throttle(uint8_t brake_pressed)
{
    return brake_pressed ? 1 : 0;
}

uint8_t safety_fault_disables_motor(mc_vehicle_mode_t mode)
{
    return (mode == VEHICLE_FAULT) ? 1 : 0;
}

uint8_t safety_limp_torque_cap(mc_vehicle_mode_t mode)
{
    if (mode == VEHICLE_LIMP) {
        return MC_TORQUE_LIMP_MAX_PERCENT; // 25%
    }
    return 255; // no cap
}

uint8_t safety_throttle_invalid(uint8_t throttle_percent)
{
    return (throttle_percent > 100) ? 1 : 0;
}

int8_t safety_clamp_torque(int8_t requested_torque,
                           mc_vehicle_mode_t mode,
                           uint8_t brake_pressed,
                           uint8_t bms_max_torque)
{
    // S1: brake overrides throttle
    if (safety_brake_overrides_throttle(brake_pressed)) {
        return 0;
    }

    // S2: FAULT mode disables motor
    if (safety_fault_disables_motor(mode)) {
        return 0;
    }

    // Determine effective max torque
    uint8_t effective_max = 100; // default: full range

    // S3: LIMP mode cap
    uint8_t limp_cap = safety_limp_torque_cap(mode);
    if (limp_cap < effective_max) {
        effective_max = limp_cap;
    }

    // BMS limit
    if (bms_max_torque < effective_max) {
        effective_max = bms_max_torque;
    }

    // Clamp: torque must be non-negative after safety processing
    if (requested_torque < 0) {
        requested_torque = 0;
    }
    if (requested_torque > (int8_t)effective_max) {
        requested_torque = (int8_t)effective_max;
    }

    return requested_torque;
}
