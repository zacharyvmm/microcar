// main.c — diagnostics tool ECU firmware
//
// The diagnostics tool sends diagnostic requests (0x600) to the gateway
// and receives responses (0x601). It supports live BMS data queries.
//
// Per costar_microcar_dogfood_plan.md Stage D.
// Uses sim_instance_state(0x4D430006) for per-instance state.
//
// Compiles as a FreeRTOS task running on the costar simulator.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include "microcar_protocol.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>
#include <stdalign.h>

#define CAN_BUS 0
#define DIAG_KEY 0x4D430006

// ── Per-instance context ──────────────────────────────────────────────────

typedef struct {
    uint8_t request_id;    // monotonically increasing
    uint8_t sent_mask;     // bitmask of sent selectors
    uint32_t script_ms;    // virtual time accumulator
} diag_ctx_t;

static diag_ctx_t *diag_ctx(void)
{
    return (diag_ctx_t *)sim_instance_state(
        DIAG_KEY, sizeof(diag_ctx_t), alignof(diag_ctx_t));
}

// ── Boot ──────────────────────────────────────────────────────────────────

void diagnostics_tool_init(void)
{
    diag_ctx_t *ctx = diag_ctx();
    ctx->request_id = 0;
    ctx->sent_mask = 0;
    ctx->script_ms = 0;
    sim_trace_u32("diag_tool_init", 1);
}

// ── Main task ─────────────────────────────────────────────────────────────

void diagnostics_tool_main(void *pvParameters)
{
    (void)pvParameters;
    diag_ctx_t *ctx = diag_ctx();
    diagnostics_tool_init();

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // Send heartbeat every 100ms.
        if (now_ms % 100 == 0) {
            mc_can_frame_t tx;
            mc_frame_init(&tx, MC_MSG_HEARTBEAT, MC_NODE_DIAGNOSTICS,
                          MC_HEARTBEAT_MSG_SIZE);
            tx.data[0] = MC_NODE_DIAGNOSTICS;
            tx.data[1] = (uint8_t)(now_ms >> 24);
            tx.data[2] = (uint8_t)(now_ms >> 16);
            tx.data[3] = (uint8_t)(now_ms >> 8);
            tx.data[4] = (uint8_t)(now_ms);
            sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
        }

        // Poll for diagnostic responses.
        uint32_t can_id, is_ext, is_remote;
        mc_can_frame_t rx;
        uint32_t dlc = sim_can_recv(CAN_BUS, rx.data, MC_MAX_PAYLOAD_SIZE,
                                    &can_id, &is_ext, &is_remote);
        if (dlc > 0) {
            rx.id = can_id;
            rx.sender = rx.data[0];
            rx.len = (uint8_t)dlc;
            if (rx.id == MC_MSG_DIAG_RESPONSE) {
                sim_trace_u32("diag_response", rx.data[2]); // request_id
            }
        }
    }
}
