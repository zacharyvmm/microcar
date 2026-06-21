// main.c — gateway ECU firmware
//
// The gateway is the central coordinator. It:
// 1. Monitors heartbeats from all ECUs
// 2. Determines the vehicle's operating mode
// 3. Aggregates faults from all sources
// 4. Broadcasts vehicle mode and warnings
//
// Compiles as a FreeRTOS task running on the costar simulator.

#include "FreeRTOS.h"
#include "task.h"

#define CAN_BUS 0

#include "gateway_state.h"
#include "heartbeat_monitor.h"
#include "fault_manager.h"
#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include "sim_abi.h"
#include <string.h>

// ── Global state ──────────────────────────────────────────────────────────

static gateway_state_t     g_gs;
static heartbeat_monitor_t g_hm;
static fault_manager_t     g_fm;

// ── Boot ──────────────────────────────────────────────────────────────────

void gateway_init(void)
{
    gateway_state_init(&g_gs);
    heartbeat_monitor_init(&g_hm, MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    fault_manager_init(&g_fm);

    // Register nodes to monitor with their heartbeat timeouts.
    heartbeat_monitor_register(&g_hm, MC_NODE_POWERTRAIN,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&g_hm, MC_NODE_BMS,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&g_hm, MC_NODE_DASHBOARD,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a heartbeat frame (0x001) from any node.
static void handle_heartbeat(uint32_t now_ms, const mc_can_frame_t *frame)
{
    uint8_t  sender = frame->sender;
    uint32_t uptime = 0;
    // Decode: first byte = node_id, next 4 = uptime_ms (big-endian)
    if (frame->len >= 5) {
        uint8_t node_id = frame->data[0];
        (void)node_id;
        uptime = ((uint32_t)frame->data[1] << 24)
               | ((uint32_t)frame->data[2] << 16)
               | ((uint32_t)frame->data[3] << 8)
               |  (uint32_t)frame->data[4];
    }

    heartbeat_monitor_beat(&g_hm, sender, now_ms);
    (void)uptime;
}

/// Process a BMS fault frame (0x202).
static void handle_bms_fault(const mc_can_frame_t *frame)
{
    uint8_t fault_code = frame->data[0];
    uint8_t severity   = fault_manager_bms_severity(fault_code);

    fault_manager_report(&g_fm, MC_NODE_BMS, fault_code, severity);
}

/// Dispatch a received CAN frame to the appropriate handler.
static void dispatch_frame(const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_HEARTBEAT:
        // Only handle heartbeats from other nodes.
        if (frame->sender != MC_NODE_GATEWAY) {
            uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
            handle_heartbeat(now_ms, frame);
        }
        break;
    case MC_MSG_BMS_FAULT:
        handle_bms_fault(frame);
        break;
    default:
        break;
    }
}

// ── Mode determination ────────────────────────────────────────────────────

/// Re-evaluate vehicle mode based on current state.
static mc_vehicle_mode_t update_vehicle_mode(uint32_t now_ms)
{
    // Check timeouts → update online/offline status.
    heartbeat_monitor_check(&g_hm, now_ms);

    uint8_t all_online      = heartbeat_monitor_all_online(&g_hm);
    uint8_t bms_fault       = fault_manager_has_critical(&g_fm);
    uint8_t bms_limp        = 0; // Set by BMS limp request
    uint8_t fault_count     = fault_manager_active_count(&g_fm);

    return gateway_state_update(&g_gs, all_online, bms_fault,
                                bms_limp, fault_count);
}

// ── CAN frame construction ────────────────────────────────────────────────

/// Build and send the heartbeat frame.
static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_GATEWAY,
                  MC_HEARTBEAT_MSG_SIZE);
    // Encode: node_id=1, uptime_ms in big-endian.
    tx->data[0] = MC_NODE_GATEWAY;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

/// Build and send the vehicle mode frame.
static void send_vehicle_mode(mc_can_frame_t *tx)
{
    mc_vehicle_mode_t mode     = g_gs.mode;
    uint8_t           fault_cd = g_fm.critical_count > 0 ? 1 : 0;

    mc_frame_init(tx, MC_MSG_VEHICLE_MODE, MC_NODE_GATEWAY,
                  MC_VEHICLE_MODE_MSG_SIZE);
    tx->data[0] = (uint8_t)mode;
    tx->data[1] = fault_cd;
}

// ── Main loop ─────────────────────────────────────────────────────────────

/// Gateway main task entry point.
///
/// Runs every 10ms virtual time.  Receives CAN frames, processes
/// heartbeats and faults, updates vehicle mode, and broadcasts status.
void gateway_main(void *pvParameters)
{
    (void)pvParameters;
    gateway_init();

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t rx;
    mc_can_frame_t tx;

    // Send initial heartbeat at boot.
    send_heartbeat(0, &tx);
    sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // ── Receive phase ─────────────────────────────────────
        uint32_t can_id;
        uint32_t is_ext;
        uint32_t is_remote;
        while (1) {
            rx.len = (uint8_t)MC_MAX_PAYLOAD_SIZE;
            uint32_t dlc = sim_can_recv(0, rx.data, MC_MAX_PAYLOAD_SIZE,
                                        &can_id, &is_ext, &is_remote);
            if (dlc == 0) break;

            rx.id = can_id;
            rx.sender = rx.data[0]; // First byte is sender node ID
            dispatch_frame(&rx);
        }

        // ── Process phase ─────────────────────────────────────
        // Check heartbeats for timeouts.
        int transitions = heartbeat_monitor_check(&g_hm, now_ms);
        if (transitions > 0) {
            uint8_t lost_node = heartbeat_monitor_last_transition_node(&g_hm);
            if (lost_node == MC_NODE_BMS) {
                // BMS lost → report fault.
                fault_manager_report(&g_fm, MC_NODE_BMS,
                                     MC_BMS_FAULT_COMM_ERROR, 2);
            }
        }

        // ── Mode update ───────────────────────────────────────
        mc_vehicle_mode_t old_mode = g_gs.mode;
        mc_vehicle_mode_t new_mode = update_vehicle_mode(now_ms);

        if (new_mode != old_mode) {
            const char *mode_str = gateway_mode_string(new_mode);
            sim_trace_u32("vehicle_mode", (uint32_t)(uintptr_t)mode_str);
        }

        // ── Broadcast phase ───────────────────────────────────
        // Send heartbeat every 100ms.
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }

        // Send vehicle mode on change or every 50ms.
        if (new_mode != old_mode || now_ms % 50 == 0) {
            send_vehicle_mode(&tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
