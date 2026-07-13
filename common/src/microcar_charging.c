// microcar_charging.c — charging FSM implementation (stub)
// Full implementation per costar_microcar_dogfood_plan.md Stage E1
// TODO: implement pure FSM transitions per the plan spec

#include "microcar_charging.h"
#include "microcar_protocol.h"

void mc_charging_init(mc_charge_state_t *cs) {
    *cs = MC_CHARGE_DISCONNECTED;
}

int mc_charging_step(mc_charge_state_t *cs, mc_charging_event_t event,
                     mc_charging_output_t *output) {
    output->next_state = *cs;
    output->command_current_a_x2 = 0;
    output->reject_reason = 0;
    return 0;
}

uint8_t mc_charging_bms_limit_for_temp(int16_t temp_c_x10) {
    if (temp_c_x10 >= 600) return 0;
    if (temp_c_x10 >= 450) return 32;
    return 64;
}

uint8_t mc_charging_clamp_target_soc(uint8_t target_soc) {
    if (target_soc < 50) return 50;
    if (target_soc > 100) return 100;
    return target_soc;
}

mc_vehicle_mode_t mc_charging_vehicle_mode(const mc_charge_state_t *cs) {
    switch (*cs) {
    case MC_CHARGE_DISCONNECTED:
        return VEHICLE_OFF;
    case MC_CHARGE_FAULT:
        return VEHICLE_FAULT;
    default:
        return VEHICLE_CHARGING;
    }
}
