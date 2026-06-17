// bms_state.h — BMS temperature-based state machine
//
// Reads battery temperature and determines the BMS state:
//   BMS_OK           — temp ≤ 60°C
//   BMS_WARN_HOT     — 60°C < temp ≤ 75°C
//   BMS_LIMP_REQUEST — 75°C < temp ≤ 90°C
//   BMS_CRITICAL_FAULT — temp > 90°C

#ifndef BMS_STATE_H
#define BMS_STATE_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// BMS state machine.
typedef struct {
    /// Current BMS state.
    mc_bms_state_t state;

    /// Last known battery temperature (0.1°C).
    int16_t temp_c_x10;

    /// Last known pack voltage (mV).
    uint16_t voltage_mv;

    /// Last known pack current (mA).
    int16_t current_ma;

    /// State of charge (percent).
    uint8_t soc_percent;

    /// Most recent fault code (MC_BMS_FAULT_NONE if no fault).
    uint8_t fault_code;

    /// Set when charger is plugged in.
    uint8_t charger_plugged;
} bms_state_t;

/// Initialise the BMS state machine.
void bms_state_init(bms_state_t *bs);

/// Update BMS state from sensor readings.
/// Returns the new BMS state.
mc_bms_state_t bms_state_update(bms_state_t *bs,
                                int16_t temp_c_x10,
                                uint16_t voltage_mv,
                                int16_t current_ma,
                                uint8_t soc_percent);

/// Determine BMS state from temperature alone.
mc_bms_state_t bms_state_from_temp(int16_t temp_c_x10);

/// Returns 1 if the BMS is in a fault state (CRITICAL_FAULT).
uint8_t bms_state_is_fault(const bms_state_t *bs);

/// Returns 1 if the BMS is requesting limp mode.
uint8_t bms_state_is_limp(const bms_state_t *bs);

/// Returns the appropriate BMS fault code for the current state.
uint8_t bms_state_fault_code(const bms_state_t *bs);

#ifdef __cplusplus
}
#endif

#endif // BMS_STATE_H
