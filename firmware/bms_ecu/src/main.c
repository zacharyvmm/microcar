// main.c — BMS ECU firmware
//
// The Battery Management System monitors the battery and:
// 1. Reads temperature, voltage, current, SOC from sensors/plant
// 2. Determines BMS state (OK/WARN/LIMP/CRITICAL)
// 3. Publishes BMS limits and fault codes
// 4. Sends periodic heartbeats
//
// Compiles as a FreeRTOS task running on the costar simulator.

#include "bms_state.h"
#include "bms_limits.h"
#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>

// ── Global state ──────────────────────────────────────────────────────────

static bms_state_t g_bs;
static bms_limits_t g_bl;

// ── Boot ──────────────────────────────────────────────────────────────────

void bms_init(void)
{
    bms_state_init(&g_bs);
    bms_limits_init(&g_bl);
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process BMS status frame (0x300) from the plant.
/// Format: [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo]
static void handle_bms_status(const mc_can_frame_t *frame)
{
    if (frame->len < 7) return;

    uint8_t  soc_percent = frame->data[0];
    uint16_t voltage_mv  = ((uint16_t)frame->data[1] << 8) | frame->data[2];
    int16_t  temp_c_x10  = (int16_t)(((uint16_t)frame->data[3] << 8) | frame->data[4]);
    int16_t  current_ma  = (int16_t)(((uint16_t)frame->data[5] << 8) | frame->data[6]);

    mc_bms_state_t old_state = g_bs.state;
    mc_bms_state_t new_state = bms_state_update(&g_bs, temp_c_x10, voltage_mv,
                                                  current_ma, soc_percent);

    // Update limits based on new state.
    bms_limits_compute(&g_bl, new_state);

    // Detect state transitions for fault reporting.
    if (new_state == BMS_CRITICAL_FAULT && old_state != BMS_CRITICAL_FAULT) {
        g_bs.fault_code = MC_BMS_FAULT_OVERTEMP;
    }
}

// ── CAN frame construction ────────────────────────────────────────────────

static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_BMS,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_BMS;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

static void send_bms_limits(mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_BMS_LIMITS, MC_NODE_BMS,
                  MC_BMS_LIMITS_MSG_SIZE);
    tx->data[0] = g_bl.max_torque_percent;
    tx->data[1] = g_bl.reason;
}

static void send_bms_fault(mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_BMS_FAULT, MC_NODE_BMS, 1);
    tx->data[0] = g_bs.fault_code;
}

// ── Main loop ─────────────────────────────────────────────────────────────

void bms_main(void)
{
    bms_init();

    uint32_t uptime_ms = 0;
    mc_can_frame_t tx;
    uint8_t last_fault_published = MC_BMS_FAULT_NONE;

    send_heartbeat(0, &tx);
    // tx → CAN controller

    while (1) {
        // vTaskDelay(pdMS_TO_TICKS(10));
        uptime_ms += 10;

        // ── Publish BMS limits every 50ms ─────────────────────
        if (uptime_ms % 50 == 0) {
            send_bms_limits(&tx);
            // sim_can_send(&tx);
        }

        // ── Publish BMS fault on change ───────────────────────
        if (g_bs.fault_code != last_fault_published) {
            send_bms_fault(&tx);
            // sim_can_send(&tx);
            last_fault_published = g_bs.fault_code;
        }

        // ── Send heartbeat ────────────────────────────────────
        if (uptime_ms % 100 == 0) {
            send_heartbeat(uptime_ms, &tx);
            // sim_can_send(&tx);
        }
    }
}
