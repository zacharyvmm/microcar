// main.c — telematics ECU firmware
//
// The telematics ECU manages vehicle-to-cloud communication. It:
// 1. Sends periodic network telemetry records (length-prefixed, big-endian)
// 2. Polls for incoming cloud commands
// 3. Sends periodic heartbeats so the gateway can track it
//
// Stage H: network device 0, length-prefixed big-endian records.
//
// Uses sim_instance_state(0x4D430008, ...) for per-instance state so
// the telematics ECU can run concurrently in multiple in-process World
// instances.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include "microcar_protocol.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include "microcar_telematics.h"
#include <string.h>
#include <stdalign.h>

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define TELEM_KEY  0x4D430008
#define CAN_BUS    0

// ── Network device configuration ──────────────────────────────────────────

#define NET_DEVICE_ID     0
#define NET_MTU           1500
#define NET_RX_BUF_SIZE   1522

// ── Telematics node ID ────────────────────────────────────────────────────

#define MC_NODE_TELEMATICS  6

// ── Heartbeat period ──────────────────────────────────────────────────────

#define TELEM_HEARTBEAT_PERIOD_MS  100

// ── Telemetry MAC address ─────────────────────────────────────────────────

static const uint8_t TELEM_MAC[6] = {
    0x02, 0x00, 0x00, 0x00, 0x00, 0x02
};

// ── Per-instance context ──────────────────────────────────────────────────

typedef struct {
    uint32_t tick_count;
    uint32_t uptime_ms;
    uint32_t seq_no;
} telem_ctx_t;

/// Return the per-machine telematics context, allocating it on first call.
static telem_ctx_t *telem_ctx(void)
{
    telem_ctx_t *ctx = (telem_ctx_t *)sim_instance_state(
        TELEM_KEY, sizeof(telem_ctx_t), alignof(telem_ctx_t));
    return ctx;
}

// ── Heartbeat helper ──────────────────────────────────────────────────────

/// Build and send a heartbeat frame on the CAN bus.
static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_TELEMATICS,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_TELEMATICS;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

// ── Telemetry record (length-prefixed, big-endian) ────────────────────────

/// Build a telemetry record in buf, returning total bytes written.
/// Format: [2-byte BE length] [4-byte BE seq_no] [4-byte BE uptime_ms]
///         [1-byte node_id]
static uint32_t build_telemetry_record(uint8_t *buf, uint32_t seq_no,
                                       uint32_t uptime_ms)
{
    // Payload: 4 bytes seq_no + 4 bytes uptime + 1 byte node_id = 9 bytes
    uint32_t payload_len = 9;
    // Total record = 2-byte length prefix + payload
    uint32_t total_len = 2 + payload_len;

    buf[0] = (uint8_t)(payload_len >> 8);    // length hi
    buf[1] = (uint8_t)(payload_len);          // length lo
    buf[2] = (uint8_t)(seq_no >> 24);         // seq_no BE
    buf[3] = (uint8_t)(seq_no >> 16);
    buf[4] = (uint8_t)(seq_no >> 8);
    buf[5] = (uint8_t)(seq_no);
    buf[6] = (uint8_t)(uptime_ms >> 24);      // uptime_ms BE
    buf[7] = (uint8_t)(uptime_ms >> 16);
    buf[8] = (uint8_t)(uptime_ms >> 8);
    buf[9] = (uint8_t)(uptime_ms);
    buf[10] = MC_NODE_TELEMATICS;             // node_id

    return total_len;
}

// ── Main task ─────────────────────────────────────────────────────────────

/// Telematics main task — sends periodic network telemetry and heartbeats.
void telematics_main(void *pvParameters)
{
    (void)pvParameters;

    telem_ctx_t *ctx = telem_ctx();
    memset(ctx, 0, sizeof(*ctx));

    sim_trace_u32("telematics_boot", 1);

    // Register the virtual Ethernet device.
    uint32_t reg_result = sim_eth_register(NET_DEVICE_ID, TELEM_MAC, NET_MTU);
    sim_trace_u32("eth_register", reg_result);

    mc_can_frame_t tx;
    uint8_t        net_buf[NET_MTU];
    uint8_t        rx_buf[NET_RX_BUF_SIZE];
    TickType_t     last_wake = xTaskGetTickCount();

    // Send initial heartbeat.
    send_heartbeat(0, &tx);
    sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
    sim_trace_u32("telem_hb", 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(TELEM_HEARTBEAT_PERIOD_MS));

        ctx->uptime_ms += TELEM_HEARTBEAT_PERIOD_MS;
        ctx->tick_count++;
        ctx->seq_no++;

        // ── Send telemetry record over network ────────────────────
        uint32_t rec_len = build_telemetry_record(net_buf, ctx->seq_no,
                                                   ctx->uptime_ms);
        uint32_t sent = sim_eth_send(NET_DEVICE_ID, net_buf, rec_len);
        sim_trace_u32("telem_net_sent", sent);

        // ── Poll and drain received network frames ────────────────
        while (sim_eth_poll(NET_DEVICE_ID)) {
            uint32_t rx_len = sim_eth_recv(NET_DEVICE_ID, rx_buf,
                                           NET_RX_BUF_SIZE);
            sim_trace_u32("telem_net_recv", rx_len);
        }

        // ── Poll CAN for incoming frames ──────────────────────────
        uint32_t can_id    = 0;
        uint32_t is_ext    = 0;
        uint32_t is_remote = 0;
        uint8_t  can_rx[MC_MAX_PAYLOAD_SIZE];

        uint32_t dlc = sim_can_recv(CAN_BUS, can_rx, MC_MAX_PAYLOAD_SIZE,
                                    &can_id, &is_ext, &is_remote);
        while (dlc > 0) {
            sim_trace_u32("telem_can_id", can_id);
            sim_trace_u32("telem_can_dlc", dlc);

            dlc = sim_can_recv(CAN_BUS, can_rx, MC_MAX_PAYLOAD_SIZE,
                               &can_id, &is_ext, &is_remote);
        }

        // ── Send periodic heartbeat ──────────────────────────────
        send_heartbeat(ctx->uptime_ms, &tx);
        sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
        sim_trace_u32("telem_hb", ctx->tick_count);
    }
}
