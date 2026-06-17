// torque_controller.c — powertrain torque computation implementation

#include "torque_controller.h"
#include "safety_rules.h"
#include <string.h>

void torque_controller_init(torque_controller_t *tc)
{
    memset(tc, 0, sizeof(*tc));
    tc->bms_torque_limit = 255; // no limit
    tc->motor_enable     = 1;
}

void torque_controller_set_input(torque_controller_t *tc,
                                 uint8_t throttle, uint8_t brake, uint8_t gear)
{
    tc->throttle_percent = throttle;
    tc->brake_pressed    = brake;
    tc->gear             = gear;
}

void torque_controller_set_mode(torque_controller_t *tc, mc_vehicle_mode_t mode)
{
    tc->vehicle_mode = mode;
}

void torque_controller_set_bms_limit(torque_controller_t *tc,
                                     uint8_t max_torque)
{
    tc->bms_torque_limit = max_torque;
}

int8_t torque_controller_compute(torque_controller_t *tc)
{
    // S6: ignore invalid throttle
    if (safety_throttle_invalid(tc->throttle_percent)) {
        tc->output_torque = 0;
        tc->motor_enable  = 0;
        return 0;
    }

    // Requested torque equals throttle percent
    int8_t requested = (int8_t)tc->throttle_percent;

    // Apply all safety rules
    tc->output_torque = safety_clamp_torque(requested,
                                            tc->vehicle_mode,
                                            tc->brake_pressed,
                                            tc->bms_torque_limit);

    // S2: FAULT mode disables motor entirely
    if (safety_fault_disables_motor(tc->vehicle_mode)) {
        tc->motor_enable = 0;
    } else {
        tc->motor_enable = 1;
    }

    return tc->output_torque;
}

uint8_t torque_controller_throttle_invalid(const torque_controller_t *tc)
{
    return safety_throttle_invalid(tc->throttle_percent);
}
