// main.c — OTA tool ECU firmware
//
// The OTA tool ECU manages firmware-over-the-air updates. It:
// 1. Listens for OTA command frames on CAN IDs 0x630-0x633
// 2. Sends periodic heartbeats so the gateway can track it
// 3. Maintains update session state in per-instance context
//
// Stage C protocol: CAN IDs 0x630-0x633 for OTA commands/responses.
//
// Uses sim_instance_state(0x4D430007, ...) for per-instance state so
// the OTA tool can run concurrently in multiple in-process World instances.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include "microcar_protocol.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>
#include <stdalign.h>

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define OTA_KEY  0x4D430007
#define CAN_BUS  0

// ── OTA tool protocol CAN IDs ─────────────────────────────────────────────

#define MC_MSG_OTA_COMMAND      0x630
#define MC_MSG_OTA_RESPONSE     0x631
#define MC_MSG_OTA_DATA         0x632
#define MC_MSG_OTA_STATUS       0x633

// ── OTA tool node ID ──────────────────────────────────────────────────────

#define MC_NODE_OTA_TOOL  5

// ── Heartbeat period ──────────────────────────────────────────────────────

#define OTA_HEARTBEAT_PERIOD_MS  100

// ── Per-instance context ──────────────────────────────────────────────────

typedef struct {
    uint32_t tick_count;
    uint32_t uptime_ms;
} ota_ctx_t;

/// Return the per-machine OTA tool context, allocating it on first call.
static ota_ctx_t *ota_ctx(void)
{
    ota_ctx_t *ctx = (ota_ctx_t *)sim_instance_state(
        OTA_KEY, sizeof(ota_ctx_t), alignof(ota_ctx_t));
    return ctx;
}

// ── Heartbeat helper ──────────────────────────────────────────────────────

/// Build and send a heartbeat frame on the CAN bus.
static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_OTA_TOOL,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_OTA_TOOL;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

// ── Main task ─────────────────────────────────────────────────────────────

/// OTA tool main task — sends periodic heartbeats and polls for OTA commands.
void ota_tool_main(void *pvParameters)
{
    (void)pvParameters;

    ota_ctx_t *ctx = ota_ctx();
    if (ctx == NULL) {
        sim_trace_u32("ota_tool_fatal", 1);
        vTaskSuspend(NULL);
        return;
    }
    memset(ctx, 0, sizeof(*ctx));

    sim_trace_u32("ota_tool_boot", 1);

    mc_can_frame_t tx;
    TickType_t last_wake = xTaskGetTickCount();

    // Send initial heartbeat.
    send_heartbeat(0, &tx);
    sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
    sim_trace_u32("ota_tool_hb", 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(OTA_HEARTBEAT_PERIOD_MS));

        ctx->uptime_ms += OTA_HEARTBEAT_PERIOD_MS;
        ctx->tick_count++;

        // ── Poll for OTA command frames ───────────────────────────
        uint32_t can_id    = 0;
        uint32_t is_ext    = 0;
        uint32_t is_remote = 0;
        uint8_t  rx_data[MC_MAX_PAYLOAD_SIZE];

        uint32_t dlc = sim_can_recv(CAN_BUS, rx_data, MC_MAX_PAYLOAD_SIZE,
                                    &can_id, &is_ext, &is_remote);
        while (dlc > 0) {
            // Trace the received command frame.
            sim_trace_u32("ota_cmd_id", can_id);
            sim_trace_u32("ota_cmd_dlc", dlc);
            if (dlc >= 1) sim_trace_u32("ota_cmd_byte0", rx_data[0]);

            dlc = sim_can_recv(CAN_BUS, rx_data, MC_MAX_PAYLOAD_SIZE,
                               &can_id, &is_ext, &is_remote);
        }

        // ── Send periodic heartbeat ──────────────────────────────
        send_heartbeat(ctx->uptime_ms, &tx);
        sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
        sim_trace_u32("ota_tool_hb", ctx->tick_count);
    }
}
