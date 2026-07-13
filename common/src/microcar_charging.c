// microcar_charging.c — charging FSM implementation
//
// Pure transition function per costar_microcar_dogfood_plan.md Stage E1.
// State transitions:
//   DISCONNECTED  + PLUG                  → PLUG_DETECTED
//   PLUG_DETECTED + HANDSHAKE_OK          → HANDSHAKE
//   HANDSHAKE     + fresh BMS limit > 0   → ACTIVE
//   ACTIVE        + lower nonzero limit   → LIMITED
//   ACTIVE/LIMITED + soc >= target        → COMPLETE
//   any plugged   + critical fault        → FAULT
//   any plugged   + UNPLUG                → DISCONNECTED
// All other combinations leave state unchanged, return non-zero reject.
//
// Command current = min(EVSE offered, BMS max) in 0.5 A units.

#include "microcar_charging.h"
#include "microcar_protocol.h"

static inline uint8_t min_u8(uint8_t a, uint8_t b) {
    return a < b ? a : b;
}

void mc_charging_init(mc_charge_state_t *cs) {
    *cs = MC_CHARGE_DISCONNECTED;
}

int mc_charging_step(mc_charge_state_t *cs, mc_charging_event_t event,
                     mc_charging_output_t *output) {

    switch (*cs) {
    case MC_CHARGE_DISCONNECTED:
        if (event.kind == MC_EVSE_PLUG) {
            output->next_state = MC_CHARGE_PLUG_DETECTED;
            output->command_current_a_x2 = 0;
            output->reject_reason = 0;
            return 0;
        }
        break;

    case MC_CHARGE_PLUG_DETECTED:
        if (event.kind == MC_EVSE_HANDSHAKE_OK) {
            output->next_state = MC_CHARGE_HANDSHAKE;
            output->command_current_a_x2 = 0;
            output->reject_reason = 0;
            return 0;
        }
        break;

    case MC_CHARGE_HANDSHAKE:
        if (event.fresh_bms_limit > 0) {
            output->next_state = MC_CHARGE_ACTIVE;
            output->command_current_a_x2 =
                min_u8(event.offered_current, event.fresh_bms_limit);
            output->reject_reason = 0;
            return 0;
        }
        break;

    case MC_CHARGE_ACTIVE:
        // BMS limit dropped below EVSE offering → LIMITED
        if (event.fresh_bms_limit > 0
            && event.fresh_bms_limit < event.offered_current) {
            output->next_state = MC_CHARGE_LIMITED;
            output->command_current_a_x2 = event.fresh_bms_limit;
            output->reject_reason = 0;
            return 0;
        }
        // SOC reached target → COMPLETE
        if (event.soc_percent >= event.target_soc) {
            output->next_state = MC_CHARGE_COMPLETE;
            output->command_current_a_x2 = 0;
            output->reject_reason = 0;
            return 0;
        }
        break;

    case MC_CHARGE_LIMITED:
        // SOC reached target → COMPLETE
        if (event.soc_percent >= event.target_soc) {
            output->next_state = MC_CHARGE_COMPLETE;
            output->command_current_a_x2 = 0;
            output->reject_reason = 0;
            return 0;
        }
        break;

    case MC_CHARGE_COMPLETE:
    case MC_CHARGE_FAULT:
        // No per-state transitions; handled by catch-all below.
        break;
    }

    // ── Catch-all transitions for any plugged-in state ─────────────────

    if (*cs != MC_CHARGE_DISCONNECTED) {
        // Critical fault → FAULT (overrides any plugged state)
        if (event.critical_fault) {
            output->next_state = MC_CHARGE_FAULT;
            output->command_current_a_x2 = 0;
            output->reject_reason = 0;
            return 0;
        }
        // UNPLUG → DISCONNECTED
        if (event.kind == MC_EVSE_UNPLUG) {
            output->next_state = MC_CHARGE_DISCONNECTED;
            output->command_current_a_x2 = 0;
            output->reject_reason = 0;
            return 0;
        }
    }

    // ── No matching transition — reject ────────────────────────────────

    output->next_state = *cs;
    output->command_current_a_x2 = 0;
    output->reject_reason = 1;
    return 1;
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
