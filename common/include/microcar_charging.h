// microcar_charging.h — charging FSM definitions (Stage E1)
//
// Types are defined in microcar_protocol.h. This header declares the
// pure transition functions.
//
// Per costar_microcar_dogfood_plan.md Stage E

#ifndef MICROCAR_CHARGING_H
#define MICROCAR_CHARGING_H

#include "microcar_protocol.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ── Charging event (input to the transition function) ────────────────────

typedef struct {
    mc_evse_event_t kind;
    uint8_t         fresh_bms_limit;   // BMS current limit in 0.5A units, 0 if stale
    uint8_t         critical_fault;    // 1 = BMS critical fault active
    uint8_t         target_soc;        // EVSE target SOC (50-100)
    uint8_t         offered_current;   // EVSE offered current in 0.5A units
} mc_charging_event_t;

// ── Charging output ──────────────────────────────────────────────────────

typedef struct {
    mc_charge_state_t next_state;
    uint8_t           command_current_a_x2;  // 0.5A units
    uint8_t           reject_reason;         // 0 = none
} mc_charging_output_t;

// ── BMS current limit thresholds ─────────────────────────────────────────

#define MC_CHARGE_BMS_FULL  64   // 32 A (< 45.0°C)
#define MC_CHARGE_BMS_WARM  32   // 16 A (45.0-59.9°C)
#define MC_CHARGE_BMS_NONE  0    //  0 A (≥ 60.0°C or critical fault)

// ── Functions ────────────────────────────────────────────────────────────

/// Initialise charging state to DISCONNECTED.
void mc_charging_init(mc_charge_state_t *cs);

/// Pure transition function.
int mc_charging_step(mc_charge_state_t *cs, mc_charging_event_t event,
                     mc_charging_output_t *output);

/// BMS current limit for a given temperature (0.1°C units).
uint8_t mc_charging_bms_limit_for_temp(int16_t temp_c_x10);

/// Clamp target SOC to [50, 100].
uint8_t mc_charging_clamp_target_soc(uint8_t target_soc);

/// Vehicle mode implied by the charging state.
mc_vehicle_mode_t mc_charging_vehicle_mode(const mc_charge_state_t *cs);

#ifdef __cplusplus
}
#endif

#endif // MICROCAR_CHARGING_H
