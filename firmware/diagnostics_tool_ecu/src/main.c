// main.c — diagnostics tool firmware
//
// A small service-tool ECU for the dogfood diagnostics lane. It sends a fixed
// request script and traces gateway responses. Scenarios can inject faults or
// driver input around the script to prove SERVICE mode and DTC behavior.
//
// Per-instance state is allocated via sim_instance_state (key 0x4D430006)
// so multiple in-process World instances each get independent cursors.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

#include "microcar_protocol.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>
#include <stdalign.h>

#define CAN_BUS 0

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define DIAG_TOOL_KEY 0x4D430006

/// Context structure holding all mutable ECU state.
typedef struct {
    uint32_t sent_mask;
    uint32_t script_ms;
} diag_tool_ctx_t;

/// Return the per-machine diagnostics tool context, allocating it on first call.
static diag_tool_ctx_t *diag_tool_ctx(void)
{
    diag_tool_ctx_t *ctx = (diag_tool_ctx_t *)sim_instance_state(
        DIAG_TOOL_KEY, sizeof(diag_tool_ctx_t), alignof(diag_tool_ctx_t));
    return ctx;
}

// ── Helpers ────────────────────────────────────────────────────────────────

static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_DIAGNOSTICS,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_DIAGNOSTICS;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

static void send_diag_request(uint8_t service, uint8_t request_id, uint8_t param,
                              mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_DIAG_REQUEST, MC_NODE_DIAGNOSTICS,
                  MC_DIAG_REQUEST_MSG_SIZE);
    tx->data[0] = MC_NODE_DIAGNOSTICS;
    tx->data[1] = service;
    tx->data[2] = request_id;
    tx->data[3] = param;
    sim_can_send(CAN_BUS, tx->id, tx->data, tx->len, 0, 0);
}

static void handle_diag_response(const mc_can_frame_t *frame)
{
    if (frame->len < MC_DIAG_RESPONSE_MSG_SIZE) return;

    uint8_t service = frame->data[1];
    uint8_t request_id = frame->data[2];
    uint8_t status = frame->data[3];
    uint8_t value0 = frame->data[4];
    uint8_t value1 = frame->data[5];

    uint32_t packed = ((uint32_t)request_id << 24)
                    | ((uint32_t)service << 16)
                    | ((uint32_t)status << 8)
                    | (uint32_t)value0;
    sim_trace_u32("diag_response", packed);

    switch (service) {
    case MC_DIAG_START_SESSION:
        sim_trace_u32("service_mode", value0);
        break;
    case MC_DIAG_READ_MODE:
        sim_trace_u32("read_mode", value0);
        break;
    case MC_DIAG_READ_DTCS:
        sim_trace_u32("dtc_count", value0);
        if (value0 > 0) sim_trace_u32("dtc_code", value1);
        break;
    case MC_DIAG_CLEAR_DTCS:
        sim_trace_u32("dtc_cleared", 1);
        break;
    default:
        break;
    }
}

// ── Main loop ──────────────────────────────────────────────────────────────

void diagnostics_tool_main(void *pvParameters)
{
    (void)pvParameters;

    diag_tool_ctx_t *ctx = diag_tool_ctx();
    if (ctx == NULL) {
        sim_trace_u32("diag_tool_fatal", 1);
        vTaskSuspend(NULL);
        return;
    }

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;

    send_heartbeat(0, &tx);
    sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        ctx->script_ms += 10;

        if (ctx->script_ms >= 100 && !(ctx->sent_mask & 0x01)) {
            send_diag_request(MC_DIAG_START_SESSION, 1, 0, &tx);
            ctx->sent_mask |= 0x01;
        }
        if (ctx->script_ms >= 200 && !(ctx->sent_mask & 0x02)) {
            send_diag_request(MC_DIAG_READ_MODE, 2, 0, &tx);
            ctx->sent_mask |= 0x02;
        }
        if (ctx->script_ms >= 350 && !(ctx->sent_mask & 0x04)) {
            send_diag_request(MC_DIAG_ACTUATOR_TEST, 6, 0, &tx);
            ctx->sent_mask |= 0x04;
        }
        if (ctx->script_ms >= 450 && !(ctx->sent_mask & 0x08)) {
            send_diag_request(MC_DIAG_READ_DTCS, 3, 0, &tx);
            ctx->sent_mask |= 0x08;
        }
        if (ctx->script_ms >= 650 && !(ctx->sent_mask & 0x10)) {
            send_diag_request(MC_DIAG_CLEAR_DTCS, 4, 0, &tx);
            ctx->sent_mask |= 0x10;
        }
        if (ctx->script_ms >= 750 && !(ctx->sent_mask & 0x20)) {
            send_diag_request(MC_DIAG_READ_DTCS, 5, 0, &tx);
            ctx->sent_mask |= 0x20;
        }

        uint32_t can_id;
        uint32_t is_ext;
        uint32_t is_remote;
        while (1) {
            mc_can_frame_t rx;
            uint32_t dlc = sim_can_recv(CAN_BUS, rx.data, MC_MAX_PAYLOAD_SIZE,
                                        &can_id, &is_ext, &is_remote);
            if (dlc == 0) break;

            rx.id = can_id;
            rx.sender = rx.data[0];
            rx.len = (uint8_t)dlc;
            if (rx.id == MC_MSG_DIAG_RESPONSE && rx.sender == MC_NODE_GATEWAY) {
                handle_diag_response(&rx);
            }
        }

        if (ctx->script_ms % 100 == 0) {
            send_heartbeat(ctx->script_ms, &tx);
            sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
