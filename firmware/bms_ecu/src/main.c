// main.c — BMS ECU firmware
//
// The Battery Management System monitors the battery and:
// 1. Reads temperature, voltage, current, SOC from sensors/plant
// 2. Determines BMS state (OK/WARN/LIMP/CRITICAL)
// 3. Publishes BMS limits and fault codes
// 4. Sends periodic heartbeats
//
// Multi-task: bms_main (prio 2) main loop,
// calibration_task (prio 2) created/destroyed dynamically.
//
// FreeRTOS primitives: xTaskCreateStatic (pre-allocated stacks),
// vTaskDelete (dynamic calibration task lifecycle).
//
// Compiles as FreeRTOS tasks running on the costar simulator.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include "bms_state.h"
#include "bms_limits.h"
#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>
#include <stdalign.h>

#define CAN_BUS 0

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define BMS_KEY 0x4D430003

/// Context structure holding all mutable ECU state, allocated per machine via
/// sim_instance_state so multiple in-process World instances each get
/// independent state.
typedef struct {
    // Static task support.
    StackType_t        calib_stack[512];
    StaticTask_t       calib_tcb;
    TaskHandle_t       calib_task_handle;
    volatile uint8_t   calib_requested;
    volatile uint8_t   calib_done;

    // Global state.
    bms_state_t        bs;
    uint8_t            seq;
    bms_limits_t       bl;
} bms_ctx_t;

/// Return the per-machine BMS context, allocating it on first call.
static bms_ctx_t *bms_ctx(void)
{
    bms_ctx_t *ctx = (bms_ctx_t *)sim_instance_state(
        BMS_KEY, sizeof(bms_ctx_t), alignof(bms_ctx_t));
    return ctx;
}
void bms_init(bms_ctx_t *ctx)
{
    bms_state_init(&ctx->bs);
    bms_limits_init(&ctx->bl);
}

// ── Calibration task ──────────────────────────────────────────────────────

/// Dynamic calibration task. Created at runtime (xTaskCreate or
/// xTaskCreateStatic), runs one calibration cycle, then deletes itself.
///
/// Uses xTaskCreateStatic for pre-allocated stack if static alloc
/// is preferred; otherwise uses dynamic xTaskCreate.
static void calibration_task(void *pvParameters)
{
    bms_ctx_t *ctx = (bms_ctx_t *)pvParameters;
    sim_trace_u32("calib_start", 1);

    // Simulate a multi-step calibration cycle.
    TickType_t start_tick = xTaskGetTickCount();
    uint32_t step = 0;

    while (step < 3) {
        vTaskDelay(pdMS_TO_TICKS(50));
        step++;
        sim_trace_u32("calib_step", step);
    }

    uint32_t duration = (uint32_t)(xTaskGetTickCount() - start_tick);
    sim_trace_u32("calib_done", duration);

    ctx->calib_done = 1;
    ctx->calib_task_handle = NULL;

    // Delete self: the task deletes itself after completing work.
    // This exercises vTaskDelete and the task lifecycle hooks in the
    // simulator (traceTASK_DELETE → sim_task_deleted → fiber cleanup).
    vTaskDelete(NULL);
}

/// Request that a calibration task be created (if not already running).
/// Returns 1 if the task was successfully created, 0 if already running.
uint8_t bms_request_calibration(bms_ctx_t *ctx, uint8_t use_static)
{
    if (ctx->calib_task_handle != NULL) {
        return 0; // Already running.
    }

    ctx->calib_requested = 1;
    ctx->calib_done = 0;

    if (use_static) {
        // Use xTaskCreateStatic with pre-allocated stack and TCB.
        ctx->calib_task_handle = xTaskCreateStatic(
            calibration_task,
            "calibration",
            512,
            ctx,
            2,
            ctx->calib_stack,
            &ctx->calib_tcb);
    } else {
        // Use dynamic allocation.
        xTaskCreate(calibration_task, "calibration", 512,
                    ctx, 2, &ctx->calib_task_handle);
    }

    if (ctx->calib_task_handle != NULL) {
        sim_trace_u32("calib_created", (uint32_t)(use_static ? 1 : 0));
        return 1;
    }

    ctx->calib_requested = 0;
    return 0;
}

/// Returns 1 if calibration is still running.
uint8_t bms_calibration_running(bms_ctx_t *ctx)
{
    return (ctx->calib_task_handle != NULL) ? 1 : 0;
}

/// Returns 1 if the last calibration completed successfully.
uint8_t bms_calibration_done(bms_ctx_t *ctx)
{
    return ctx->calib_done;
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a plant sensor frame (0x500) into live BMS state.
/// Format: [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo]
static void handle_plant_sensors(bms_ctx_t *ctx, const mc_can_frame_t *frame)
{
    if (frame->len < 7) return;

    uint8_t  soc_percent = frame->data[0];
    uint16_t voltage_mv  = ((uint16_t)frame->data[1] << 8) | frame->data[2];
    int16_t  temp_c_x10  = (int16_t)(((uint16_t)frame->data[3] << 8) | frame->data[4]);
    int16_t  current_ma  = (int16_t)(((uint16_t)frame->data[5] << 8) | frame->data[6]);

    mc_bms_state_t old_state = ctx->bs.state;
    mc_bms_state_t new_state = bms_state_update(&ctx->bs, temp_c_x10, voltage_mv,
                                                  current_ma, soc_percent);

    // Update limits based on new state.
    bms_limits_compute(&ctx->bl, new_state);

    // Detect state transitions for fault reporting.
    if (new_state == BMS_CRITICAL_FAULT && old_state != BMS_CRITICAL_FAULT) {
        ctx->bs.fault_code = MC_BMS_FAULT_OVERTEMP;
    }

    // Increment wrapping sequence and publish BMS status.
    ctx->seq++;
    {
        mc_can_frame_t tx;
        mc_frame_init(&tx, MC_MSG_BMS_STATUS, MC_NODE_BMS,
                      MC_BMS_STATUS_MSG_SIZE);
        mc_bms_status_msg_t status = {
            .pack_voltage_mv = voltage_mv,
            .pack_current_ma = current_ma,
            .pack_temp_c_x10 = temp_c_x10,
            .soc_percent = soc_percent,
            .seq = ctx->seq,
        };
        memcpy(tx.data, &status, MC_BMS_STATUS_MSG_SIZE);
        sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
    }
}

/// Dispatch a received CAN frame to the appropriate handler.
static void dispatch_frame(bms_ctx_t *ctx, const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_PLANT_SENSORS: /* plant → BMS sensor path (0x500) — the only
                                 * plant-decoder input. MC_MSG_BMS_STATUS
                                 * (0x200) is this ECU's own output and uses a
                                 * different wire layout; it must never be fed
                                 * back through the plant decoder. */
        handle_plant_sensors(ctx, frame);
        break;
    default:
        break;
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

static void send_bms_limits(bms_ctx_t *ctx, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_BMS_LIMITS, MC_NODE_BMS,
                  MC_BMS_LIMITS_MSG_SIZE);
    tx->data[0] = ctx->bl.max_torque_percent;
    tx->data[1] = ctx->bl.reason;
}

static void send_bms_fault(bms_ctx_t *ctx, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_BMS_FAULT, MC_NODE_BMS, 1);
    tx->data[0] = ctx->bs.fault_code;
}

// ── Main loop ─────────────────────────────────────────────────────────────

void bms_main(void *pvParameters)
{
    (void)pvParameters;
    bms_ctx_t *ctx = bms_ctx();
    if (ctx == NULL) {
        sim_trace_u32("bms_fatal", 1);
        vTaskSuspend(NULL);
        return;
    }
    bms_init(ctx);

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;
    uint8_t last_fault_published = MC_BMS_FAULT_NONE;

    send_heartbeat(0, &tx);
    sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);

    // At boot time, start a static calibration.
    bms_request_calibration(ctx, 1);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // ── Calibration lifecycle management ────────────────────
        // After calibration completes, request a new dynamic one
        // at t=2000ms to demonstrate repeated create/delete cycles.
        if (bms_calibration_done(ctx) && !bms_calibration_running(ctx) &&
            now_ms >= 2000 && now_ms < 2500) {
            bms_request_calibration(ctx, 0); // dynamic create
            sim_trace_u32("calib_restart", now_ms);
        }

        // ── Receive phase ─────────────────────────────────────
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
            dispatch_frame(ctx, &rx);
        }

        // ── Publish BMS limits every 50ms ─────────────────────
        if (now_ms % 50 == 0) {
            send_bms_limits(ctx, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }

        // ── Publish BMS fault on change ───────────────────────
        if (ctx->bs.fault_code != last_fault_published) {
            send_bms_fault(ctx, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
            last_fault_published = ctx->bs.fault_code;
        }

        // ── Send heartbeat ────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
