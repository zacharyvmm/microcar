// main.c — gateway ECU firmware
//
// The gateway is the central coordinator. It:
// 1. Monitors heartbeats from all ECUs
// 2. Determines the vehicle's operating mode
// 3. Aggregates faults from all sources
// 4. Broadcasts vehicle mode and warnings
//
// Multi-task: heartbeat_rx (prio 4) captures CAN → queue,
// gateway_main (prio 3) processes heartbeats/faults/mode,
// fault_aggregator (prio 2) low-rate fault aggregation.
//
// FreeRTOS primitives exercised: Mutex, Event groups, Task notifications,
// Queues (all created via xSemaphoreCreateMutex, xEventGroupCreate, etc.)
//
// Compiles as FreeRTOS tasks running on the costar simulator.

#include "FreeRTOS.h"
#include "task.h"
#include "queue.h"
#include "semphr.h"
#include "event_groups.h"
#include "timers.h"

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

// ── Heartbeat queue item ───────────────────────────────────────────────────

/// A single heartbeat event pushed from heartbeat_rx onto the queue.
typedef struct {
    uint8_t  sender;
    uint32_t uptime_ms;
} hb_event_t;

// ── Global state ──────────────────────────────────────────────────────────

static gateway_state_t     g_gs;
static heartbeat_monitor_t g_hm;
static fault_manager_t     g_fm;

// ── FreeRTOS primitives ───────────────────────────────────────────────────

/// Mutex protecting fault_manager_t (guards concurrent access from
/// gateway_main and fault_aggregator).
static SemaphoreHandle_t g_fm_mutex = NULL;

/// Event group for mode transition signalling.
/// Bits:
///   0x01 – mode changed (set by gateway_main)
///   0x02 – critical fault raised
static EventGroupHandle_t g_mode_events = NULL;

/// Queue carrying heartbeat events from heartbeat_rx → gateway_main.
/// Depth: 16 items, each sizeof(hb_event_t).
static QueueHandle_t g_hb_queue = NULL;

/// Task handle for gateway_main (receives task notifications for
/// urgent fault alerts).
static TaskHandle_t g_gateway_task_handle = NULL;

// ── Boot ──────────────────────────────────────────────────────────────────

/// Allocate FreeRTOS primitives. Called once from gateway_main.
static void gateway_primitives_init(void)
{
    g_fm_mutex     = xSemaphoreCreateMutex();
    g_mode_events  = xEventGroupCreate();
    g_hb_queue     = xQueueCreate(16, sizeof(hb_event_t));

    sim_trace_u32("gateway_mutex", (uint32_t)(uintptr_t)g_fm_mutex);
    sim_trace_u32("gateway_event_group", (uint32_t)(uintptr_t)g_mode_events);
    sim_trace_u32("gateway_queue", (uint32_t)(uintptr_t)g_hb_queue);
}

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
/// Called from gateway_main after dequeuing an hb_event_t.
static void handle_heartbeat(uint32_t now_ms, const hb_event_t *ev)
{
    heartbeat_monitor_beat(&g_hm, ev->sender, now_ms);
    (void)ev->uptime_ms;
}

/// Process a BMS fault frame (0x202).
/// Protected by mutex.
static void handle_bms_fault(const mc_can_frame_t *frame)
{
    uint8_t fault_code = frame->data[0];
    uint8_t severity   = fault_manager_bms_severity(fault_code);

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_manager_report(&g_fm, MC_NODE_BMS, fault_code, severity);

        // If critical fault, notify gateway_main immediately.
        if (severity == 2 && g_gateway_task_handle != NULL) {
            xTaskNotify(g_gateway_task_handle,
                        (uint32_t)fault_code,
                        eSetValueWithoutOverwrite);
        }

        xSemaphoreGive(g_fm_mutex);
    }
}

/// Dispatch a received CAN frame to the appropriate handler.
/// Called from heartbeat_rx task (only processes heartbeat and BMS fault).
static void dispatch_frame_in_rx(const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_HEARTBEAT:
        // Forward to heartbeat monitor via queue.
        if (frame->sender != MC_NODE_GATEWAY) {
            hb_event_t ev;
            ev.sender = frame->sender;
            ev.uptime_ms = 0;
            if (frame->len >= 5) {
                ev.uptime_ms = ((uint32_t)frame->data[1] << 24)
                             | ((uint32_t)frame->data[2] << 16)
                             | ((uint32_t)frame->data[3] << 8)
                             |  (uint32_t)frame->data[4];
            }
            xQueueSend(g_hb_queue, &ev, 0);
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
/// Protected by mutex for fault_manager access.
static mc_vehicle_mode_t update_vehicle_mode(uint32_t now_ms)
{
    // Check timeouts → update online/offline status.
    heartbeat_monitor_check(&g_hm, now_ms);

    uint8_t all_online      = heartbeat_monitor_all_online(&g_hm);
    uint8_t bms_fault       = 0;
    uint8_t bms_limp        = 0;
    uint8_t fault_count     = 0;

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        bms_fault   = fault_manager_has_critical(&g_fm);
        fault_count = fault_manager_active_count(&g_fm);
        xSemaphoreGive(g_fm_mutex);
    }

    return gateway_state_update(&g_gs, all_online, bms_fault,
                                bms_limp, fault_count);
}

// ── CAN frame construction ────────────────────────────────────────────────

/// Build and send the heartbeat frame.
static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_GATEWAY,
                  MC_HEARTBEAT_MSG_SIZE);
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
    uint8_t           fault_cd = 0;

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_cd = g_fm.critical_count > 0 ? 1 : 0;
        xSemaphoreGive(g_fm_mutex);
    }

    mc_frame_init(tx, MC_MSG_VEHICLE_MODE, MC_NODE_GATEWAY,
                  MC_VEHICLE_MODE_MSG_SIZE);
    tx->data[0] = (uint8_t)mode;
    tx->data[1] = fault_cd;
}

// ── Heartbeat RX task ─────────────────────────────────────────────────────

/// High-priority task that drains CAN RX and pushes heartbeat events
/// onto g_hb_queue.  Also handles BMS fault frames inline.
void heartbeat_rx(void *pvParameters)
{
    (void)pvParameters;
    sim_register_symbol((uint64_t)xTaskGetCurrentTaskHandle(), "heartbeat_rx");

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(5));

        uint32_t can_id;
        uint32_t is_ext;
        uint32_t is_remote;
        while (1) {
            mc_can_frame_t rx;
            uint32_t dlc = sim_can_recv(0, rx.data, MC_MAX_PAYLOAD_SIZE,
                                        &can_id, &is_ext, &is_remote);
            if (dlc == 0) break;

            rx.id = can_id;
            rx.sender = rx.data[0];
            rx.len = (uint8_t)dlc;
            dispatch_frame_in_rx(&rx);
        }
    }
}

// ── Fault aggregator task ──────────────────────────────────────────────────

/// Low-rate task that aggregates fault statistics and traces them.
/// Accesses fault_manager under mutex protection.
void fault_aggregator(void *pvParameters)
{
    (void)pvParameters;
    sim_register_symbol((uint64_t)xTaskGetCurrentTaskHandle(), "fault_aggregator");

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        uint8_t critical = 0;
        uint8_t active   = 0;
        uint8_t warning  = 0;

        if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(10)) == pdTRUE) {
            critical = g_fm.critical_count;
            warning  = g_fm.warning_count;
            active   = fault_manager_active_count(&g_fm);
            xSemaphoreGive(g_fm_mutex);
        }

        // Trace aggregated fault stats.
        uint32_t agg = ((uint32_t)critical << 16) | ((uint32_t)warning << 8) | active;
        sim_trace_u32("fault_aggregate", agg);

        // If critical fault exists, set event group bit.
        if (critical > 0) {
            xEventGroupSetBits(g_mode_events, 0x02);
        }
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

/// Gateway main task entry point.
///
/// Runs every 10ms virtual time.  Dequeues heartbeat events from
/// heartbeat_rx, processes faults, updates vehicle mode, and broadcasts
/// status.  Also checks event group for mode changes and task
/// notifications for urgent faults.
void gateway_main(void *pvParameters)
{
    (void)pvParameters;
    gateway_init();
    gateway_primitives_init();
    sim_register_symbol((uint64_t)xTaskGetCurrentTaskHandle(), "gateway_main");
    g_gateway_task_handle = xTaskGetCurrentTaskHandle();

    // Create subordinate tasks.
    xTaskCreate(heartbeat_rx, "hb_rx", 768, NULL, 4, NULL);
    xTaskCreate(fault_aggregator, "fault_agg", 512, NULL, 2, NULL);

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;

    // Send initial heartbeat at boot.
    send_heartbeat(0, &tx);
    sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // ── Dequeue heartbeat events ────────────────────────────
        hb_event_t ev;
        while (xQueueReceive(g_hb_queue, &ev, 0) == pdTRUE) {
            handle_heartbeat(now_ms, &ev);
        }

        // ── Check for urgent fault notifications ────────────────
        uint32_t notify_val = 0;
        if (xTaskNotifyWait(0, 0xFFFFFFFF, &notify_val, 0) == pdTRUE) {
            sim_trace_u32("urgent_fault_notify", notify_val);
        }

        // ── Process phase: check heartbeats for timeouts ────────
        int transitions = heartbeat_monitor_check(&g_hm, now_ms);
        if (transitions > 0) {
            uint8_t lost_node = heartbeat_monitor_last_transition_node(&g_hm);
            if (lost_node == MC_NODE_BMS) {
                // BMS lost → report fault.
                if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                    fault_manager_report(&g_fm, MC_NODE_BMS,
                                         MC_BMS_FAULT_COMM_ERROR, 2);
                    xSemaphoreGive(g_fm_mutex);
                }
            }
        }

        // ── Mode update ─────────────────────────────────────────
        mc_vehicle_mode_t old_mode = g_gs.mode;
        mc_vehicle_mode_t new_mode = update_vehicle_mode(now_ms);

        if (new_mode != old_mode) {
            const char *mode_str = gateway_mode_string(new_mode);
            sim_trace_u32("vehicle_mode", (uint32_t)(uintptr_t)mode_str);
            // Signal mode change to event group.
            xEventGroupSetBits(g_mode_events, 0x01);
        }

        // ── Check event group for mode transitions ──────────────
        // (this lets other tasks or tests wait for mode changes)
        EventBits_t mode_bits = xEventGroupGetBits(g_mode_events);
        if (mode_bits & 0x01) {
            sim_trace_u32("mode_event_group", mode_bits);
            xEventGroupClearBits(g_mode_events, 0x01);
        }

        // ── Broadcast phase ─────────────────────────────────────
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
