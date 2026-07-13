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
//
// B2: All mutable state migrated to sim_instance_state(0x4D430001, ...) so
// the gateway can run concurrently in multiple in-process World instances
// with independent device banks (UNBLOCKING.md §B2).

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
#include <stdalign.h>

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define GW_KEY 0x4D430001

// ── Heartbeat queue item ───────────────────────────────────────────────────

/// A single heartbeat event pushed from heartbeat_rx onto the queue.
typedef struct {
    uint8_t  sender;
    uint32_t uptime_ms;
} hb_event_t;

// ── Per-instance context (replaces all file-scope statics) ────────────────

typedef struct {
    // Core state
    gateway_state_t     gs;
    heartbeat_monitor_t hm;
    fault_manager_t     fm;

    // FreeRTOS primitives
    SemaphoreHandle_t   fm_mutex;
    EventGroupHandle_t  mode_events;
    QueueHandle_t       hb_queue;
    SemaphoreHandle_t   can_frame_sem;

    // Task handle for gateway_main (receives task notifications)
    TaskHandle_t        gateway_task_handle;
} gateway_ctx_t;

/// Return the per-machine gateway context, allocating it on first call.
static gateway_ctx_t *gateway_ctx(void)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)sim_instance_state(
        GW_KEY, sizeof(gateway_ctx_t), alignof(gateway_ctx_t));
    return ctx;
}

// ── Boot ──────────────────────────────────────────────────────────────────

/// Allocate FreeRTOS primitives. Called once from gateway_main.
static void gateway_primitives_init(gateway_ctx_t *ctx)
{
    ctx->fm_mutex       = xSemaphoreCreateMutex();
    ctx->mode_events    = xEventGroupCreate();
    ctx->hb_queue       = xQueueCreate(16, sizeof(hb_event_t));
    ctx->can_frame_sem  = xSemaphoreCreateCounting(64, 0);

    sim_trace_u32("gateway_mutex", ctx->fm_mutex != NULL ? 1 : 0);
    sim_trace_u32("gateway_event_group", ctx->mode_events != NULL ? 1 : 0);
    sim_trace_u32("gateway_queue", ctx->hb_queue != NULL ? 1 : 0);
    sim_trace_u32("gateway_can_sem", ctx->can_frame_sem != NULL ? 1 : 0);
}

void gateway_init(void)
{
    gateway_ctx_t *ctx = gateway_ctx();

    gateway_state_init(&ctx->gs);
    heartbeat_monitor_init(&ctx->hm, MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    fault_manager_init(&ctx->fm);

    // Register nodes to monitor with their heartbeat timeouts.
    heartbeat_monitor_register(&ctx->hm, MC_NODE_POWERTRAIN,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&ctx->hm, MC_NODE_BMS,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&ctx->hm, MC_NODE_DASHBOARD,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a heartbeat frame (0x001) from any node.
/// Called from gateway_main after dequeuing an hb_event_t.
static void handle_heartbeat(gateway_ctx_t *ctx, uint32_t now_ms,
                             const hb_event_t *ev)
{
    heartbeat_monitor_beat(&ctx->hm, ev->sender, now_ms);
    (void)ev->uptime_ms;
}

/// Process a BMS fault frame (0x202).
/// Protected by mutex.
static void handle_bms_fault(gateway_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t fault_code = frame->data[0];
    uint8_t severity   = fault_manager_bms_severity(fault_code);

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_manager_report(&ctx->fm, MC_NODE_BMS, fault_code, severity);

        // If critical fault, notify gateway_main immediately.
        if (severity == 2 && ctx->gateway_task_handle != NULL) {
            xTaskNotify(ctx->gateway_task_handle,
                        (uint32_t)fault_code,
                        eSetValueWithoutOverwrite);
        }

        xSemaphoreGive(ctx->fm_mutex);
    }
}

/// Dispatch a received CAN frame to the appropriate handler.
/// Called from heartbeat_rx task (only processes heartbeat and BMS fault).
static void dispatch_frame_in_rx(gateway_ctx_t *ctx,
                                 const mc_can_frame_t *frame)
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
            xQueueSend(ctx->hb_queue, &ev, 0);
        }
        break;
    case MC_MSG_BMS_FAULT:
        handle_bms_fault(ctx, frame);
        break;
    default:
        break;
    }
}

// ── Mode determination ────────────────────────────────────────────────────

/// Re-evaluate vehicle mode based on current state.
/// Protected by mutex for fault_manager access.
static mc_vehicle_mode_t update_vehicle_mode(gateway_ctx_t *ctx,
                                             uint32_t now_ms)
{
    // Check timeouts → update online/offline status.
    heartbeat_monitor_check(&ctx->hm, now_ms);

    uint8_t all_online      = heartbeat_monitor_all_online(&ctx->hm);
    uint8_t bms_fault       = 0;
    uint8_t bms_limp        = 0;
    uint8_t fault_count     = 0;

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        bms_fault   = fault_manager_has_critical(&ctx->fm);
        fault_count = fault_manager_active_count(&ctx->fm);
        xSemaphoreGive(ctx->fm_mutex);
    }

    return gateway_state_update(&ctx->gs, all_online, bms_fault,
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
static void send_vehicle_mode(gateway_ctx_t *ctx, mc_can_frame_t *tx)
{
    mc_vehicle_mode_t mode     = ctx->gs.mode;
    uint8_t           fault_cd = 0;

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_cd = ctx->fm.critical_count > 0 ? 1 : 0;
        xSemaphoreGive(ctx->fm_mutex);
    }

    mc_frame_init(tx, MC_MSG_VEHICLE_MODE, MC_NODE_GATEWAY,
                  MC_VEHICLE_MODE_MSG_SIZE);
    tx->data[0] = (uint8_t)mode;
    tx->data[1] = fault_cd;
}

// ── Heartbeat RX task ─────────────────────────────────────────────────────

/// High-priority task that drains CAN RX and pushes heartbeat events
/// onto the heartbeat queue.  Also handles BMS fault frames inline.
/// Receives gateway_ctx_t * as pvParameters.
void heartbeat_rx(void *pvParameters)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)pvParameters;

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
            dispatch_frame_in_rx(ctx, &rx);

            // Give semaphore to preempt: can_frame_processor (prio 5)
            // will wake and process this frame.
            xSemaphoreGive(ctx->can_frame_sem);
        }
    }
}

// ── Fault aggregator task ──────────────────────────────────────────────────

/// Low-rate task that aggregates fault statistics and traces them.
/// Accesses fault_manager under mutex protection.
/// Receives gateway_ctx_t * as pvParameters.
void fault_aggregator(void *pvParameters)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        uint8_t critical = 0;
        uint8_t active   = 0;
        uint8_t warning  = 0;

        if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(10)) == pdTRUE) {
            critical = ctx->fm.critical_count;
            warning  = ctx->fm.warning_count;
            active   = fault_manager_active_count(&ctx->fm);
            xSemaphoreGive(ctx->fm_mutex);
        }

        // Trace aggregated fault stats.
        uint32_t agg = ((uint32_t)critical << 16) | ((uint32_t)warning << 8) | active;
        sim_trace_u32("fault_aggregate", agg);

        // If critical fault exists, set event group bit.
        if (critical > 0) {
            xEventGroupSetBits(ctx->mode_events, 0x02);
        }
    }
}

// ── CAN frame processor task ──────────────────────────────────────────

/// Highest-priority task (5) that processes CAN frames via counting
/// semaphore.  heartbeat_rx (prio 4) gives the semaphore for each
/// received frame, which preempts lower-priority tasks so this
/// processor runs immediately.
/// Each activation traces a counter showing preemption count.
/// Receives gateway_ctx_t * as pvParameters.
void can_frame_processor(void *pvParameters)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)pvParameters;

    uint32_t proc_count = 0;

    while (1) {
        // Block until heartbeat_rx gives the semaphore.
        xSemaphoreTake(ctx->can_frame_sem, portMAX_DELAY);

        proc_count++;

        // Trace the processing event to show preemption occurred.
        // The value is the activation count — proves every give
        // caused a preemption wakeup.
        sim_trace_u32("can_frame_proc", proc_count);
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

    gateway_ctx_t *ctx = gateway_ctx();
    gateway_init();
    gateway_primitives_init(ctx);
    ctx->gateway_task_handle = xTaskGetCurrentTaskHandle();

    // Create subordinate tasks, passing the context pointer.
    xTaskCreate(heartbeat_rx, "hb_rx", 768, ctx, 4, NULL);
    xTaskCreate(fault_aggregator, "fault_agg", 512, ctx, 2, NULL);
    xTaskCreate(can_frame_processor, "can_proc", 768, ctx, 5, NULL);

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
        while (xQueueReceive(ctx->hb_queue, &ev, 0) == pdTRUE) {
            handle_heartbeat(ctx, now_ms, &ev);
        }

        // ── Check for urgent fault notifications ────────────────
        uint32_t notify_val = 0;
        if (xTaskNotifyWait(0, 0xFFFFFFFF, &notify_val, 0) == pdTRUE) {
            sim_trace_u32("urgent_fault_notify", notify_val);
        }

        // ── Process phase: check heartbeats for timeouts ────────
        int transitions = heartbeat_monitor_check(&ctx->hm, now_ms);
        if (transitions > 0) {
            uint8_t lost_node = heartbeat_monitor_last_transition_node(&ctx->hm);
            if (lost_node == MC_NODE_BMS) {
                // BMS lost → report fault.
                if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                    fault_manager_report(&ctx->fm, MC_NODE_BMS,
                                         MC_BMS_FAULT_COMM_ERROR, 2);
                    xSemaphoreGive(ctx->fm_mutex);
                }
            }
        }

        // ── Mode update ─────────────────────────────────────────
        mc_vehicle_mode_t old_mode = ctx->gs.mode;
        mc_vehicle_mode_t new_mode = update_vehicle_mode(ctx, now_ms);

        if (new_mode != old_mode) {
            sim_trace_u32("vehicle_mode", (uint32_t)new_mode);
            // Signal mode change to event group.
            xEventGroupSetBits(ctx->mode_events, 0x01);
        }

        // ── Check event group for mode transitions ──────────────
        // (this lets other tasks or tests wait for mode changes)
        EventBits_t mode_bits = xEventGroupGetBits(ctx->mode_events);
        if (mode_bits & 0x01) {
            sim_trace_u32("mode_event_group", mode_bits);
            xEventGroupClearBits(ctx->mode_events, 0x01);
        }

        // ── Broadcast phase ─────────────────────────────────────
        // Send heartbeat every 100ms.
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }

        // Send vehicle mode on change or every 50ms.
        if (new_mode != old_mode || now_ms % 50 == 0) {
            send_vehicle_mode(ctx, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
