// main.c — powertrain ECU firmware
//
// The powertrain controls the motor. It:
// 1. Receives driver input from the plant
// 2. Receives vehicle mode from the gateway
// 3. Receives BMS torque limits
// 4. Computes safe motor torque
// 5. Watches for gateway heartbeat timeout (S4)
//
// Multi-task: powertrain_main (prio 3) torque control + CAN,
// sensor_poll (prio 3) throttle/brake read,
// logger (prio 1) low-rate event logging.
//
// FreeRTOS primitives: Counting semaphore (CAN TX mailbox),
// Software timer (watchdog periodic check), vTaskDelayUntil.
//
// Compiles as FreeRTOS tasks running on the costar simulator.
//
// Per-instance state is allocated via sim_instance_state (key 0x4D430002)
// so multiple in-process World instances each get independent mutable state.

#include "FreeRTOS.h"
#include "task.h"
#include "queue.h"
#include "semphr.h"
#include "timers.h"
#include "sim_abi.h"

#define CAN_BUS 0

#include "torque_controller.h"
#include "watchdog_task.h"
#include "microcar_protocol.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>
#include <stdalign.h>

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define POWERTRAIN_KEY 0x4D430002

/// Context structure holding all mutable ECU state.
typedef struct {
    torque_controller_t tc;
    watchdog_task_t     wd;
    uint8_t             diag_force_service_mode;
    uint8_t             charging_force_mode;
    uint8_t             diag_service_clamp_bug;
    SemaphoreHandle_t   can_tx_slots;
    TimerHandle_t       wd_timer;
    TaskHandle_t        task_handle;
} powertrain_ctx_t;

/// Return the per-machine powertrain context, allocating it on first call.
static powertrain_ctx_t *powertrain_ctx(void)
{
    powertrain_ctx_t *ctx = (powertrain_ctx_t *)sim_instance_state(
        POWERTRAIN_KEY, sizeof(powertrain_ctx_t), alignof(powertrain_ctx_t));
    return ctx;
}

// ── Watchdog timer callback ───────────────────────────────────────────────

/// Software timer callback: check gateway watchdog and trace status.
static void watchdog_timer_cb(TimerHandle_t xTimer)
{
    (void)xTimer;
    // Timer callbacks don't receive a context pointer, so obtain it here.
    powertrain_ctx_t *ctx = powertrain_ctx();
    if (ctx == NULL) return;
    uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
    uint8_t timeout = watchdog_check(&ctx->wd, now_ms);

    if (timeout) {
        // S4: Gateway heartbeat lost → disable torque.
        ctx->tc.motor_enable = 0;
        sim_trace_u32("gateway_timeout", now_ms);
    } else {
        sim_trace_u32("watchdog_ok", now_ms);
    }
}

// ── Boot ──────────────────────────────────────────────────────────────────

/// Allocate FreeRTOS primitives. Called once from powertrain_main.
static void powertrain_primitives_init(powertrain_ctx_t *ctx)
{
    ctx->can_tx_slots = xSemaphoreCreateCounting(3, 3);

    ctx->wd_timer = xTimerCreate(
        "wd_timer",
        pdMS_TO_TICKS(50),
        pdTRUE,   // auto-reload
        NULL,
        watchdog_timer_cb);

    if (ctx->wd_timer != NULL) {
        xTimerStart(ctx->wd_timer, 0);
    }

    sim_trace_u32("pt_can_sem", ctx->can_tx_slots != NULL ? 1 : 0);
    sim_trace_u32("pt_wd_timer", ctx->wd_timer != NULL ? 1 : 0);
}

static void powertrain_init(powertrain_ctx_t *ctx)
{
    torque_controller_init(&ctx->tc);
    watchdog_init(&ctx->wd);
}

void powertrain_enable_dogfood_service_mode(void)
{
    powertrain_ctx_t *ctx = powertrain_ctx();
    if (ctx != NULL) ctx->diag_force_service_mode = 1;
}

void powertrain_enable_dogfood_charging(void)
{
    powertrain_ctx_t *ctx = powertrain_ctx();
    if (ctx != NULL) ctx->charging_force_mode = 1;
}

// SEEDED DEBUG-GYM BUG (service_torque): enable the buggy powertrain firmware
// that runs a SERVICE-mode torque computation but skips the safety clamp. The
// fixed reference is powertrain_enable_dogfood_service_mode
// (powertrain_diag_service), which clamps torque to 0 and disables the motor.
void powertrain_enable_dogfood_service_clamp_bug(void)
{
    powertrain_ctx_t *ctx = powertrain_ctx();
    if (ctx != NULL) {
        ctx->diag_force_service_mode = 1; // run a SERVICE-mode torque computation
        ctx->diag_service_clamp_bug  = 1; // BUG: skip the SERVICE clamp
    }
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a driver input frame (0x020) from the plant.
static void handle_driver_input(powertrain_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t throttle = frame->data[0];
    uint8_t brake    = frame->data[1];
    uint8_t gear     = frame->len >= 3 ? frame->data[2] : 0;

    torque_controller_set_input(&ctx->tc, throttle, brake, gear);
}

/// Process a vehicle mode frame (0x010) from the gateway.
static void handle_vehicle_mode(powertrain_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t mode = frame->data[0];
    torque_controller_set_mode(&ctx->tc, (mc_vehicle_mode_t)mode);
}

/// Process a BMS limits frame (0x201).
static void handle_bms_limits(powertrain_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t max_torque = frame->data[0];
    torque_controller_set_bms_limit(&ctx->tc, max_torque);
}

/// Process a gateway heartbeat frame (0x001).
static void handle_gateway_heartbeat(powertrain_ctx_t *ctx, uint32_t now_ms)
{
    watchdog_gateway_beat(&ctx->wd, now_ms);
}

/// Dispatch a received CAN frame to the appropriate handler.
static void dispatch_frame(powertrain_ctx_t *ctx, const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_DRIVER_INPUT:
        handle_driver_input(ctx, frame);
        break;
    case MC_MSG_VEHICLE_MODE:
        handle_vehicle_mode(ctx, frame);
        break;
    case MC_MSG_BMS_LIMITS:
        handle_bms_limits(ctx, frame);
        break;
    case MC_MSG_HEARTBEAT:
        if (frame->sender == MC_NODE_GATEWAY) {
            uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
            handle_gateway_heartbeat(ctx, now_ms);
        }
        break;
    default:
        break;
    }
}

// ── CAN TX with counting semaphore ────────────────────────────────────────

/// Send a CAN frame using the counting semaphore as mailbox.
/// Takes a slot, sends, then gives it back after a short delay.
static void can_tx_with_semaphore(powertrain_ctx_t *ctx, mc_can_frame_t *frame)
{
    if (xSemaphoreTake(ctx->can_tx_slots, pdMS_TO_TICKS(2)) == pdTRUE) {
        sim_can_send(0, frame->id, frame->data, frame->len, 0, 0);
        // Release the mailbox slot (simulates TX completion interrupt).
        xSemaphoreGive(ctx->can_tx_slots);
    }
}

// ── CAN frame construction ────────────────────────────────────────────────

static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_POWERTRAIN,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_POWERTRAIN;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

static void send_motor_command(powertrain_ctx_t *ctx, int8_t torque, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_MOTOR_COMMAND, MC_NODE_POWERTRAIN,
                  MC_MOTOR_COMMAND_MSG_SIZE);
    tx->data[0] = (uint8_t)torque;
    tx->data[1] = ctx->tc.motor_enable;
}

// ── Sensor poll task ──────────────────────────────────────────────────────

/// Reads throttle/brake sensor values via a simple poll.
/// Runs at 5ms period, prio 3 (same as powertrain_main).
void sensor_poll(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(5));

        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // Emit sensor poll trace every 200ms.
        if (now_ms % 200 == 0) {
            sim_trace_u32("sensor_poll_tick", now_ms);
        }
    }
}

// ── Deadline monitor task ──────────────────────────────────────────────────

/// Measures jitter between expected and actual wake time via vTaskDelayUntil.
/// If jitter > 2 ticks, traces `deadline_miss` with the jitter value.
void deadline_monitor(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(50));

        TickType_t now = xTaskGetTickCount();
        TickType_t expected = last_wake;

        int32_t jitter = (int32_t)(now - expected);
        if (jitter < 0) jitter = -jitter;

        if (jitter > 2) {
            sim_trace_u32("deadline_miss", (uint32_t)jitter);
        } else {
            sim_trace_u32("deadline_ok", (uint32_t)jitter);
        }
    }
}

// ── Logger task ───────────────────────────────────────────────────────────

/// Low-rate event logger. Runs at 100ms period, prio 1.
void logger(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
        powertrain_ctx_t *ctx = powertrain_ctx();
        if (ctx == NULL) continue;

        // Log current torque and gateway online status.
        uint8_t gw_online = watchdog_gateway_online(&ctx->wd);
        uint32_t log_val = ((uint32_t)gw_online << 16) | (ctx->tc.motor_enable & 1);
        sim_trace_u32("logger_event", log_val);

        (void)now_ms;
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

void powertrain_main(void *pvParameters)
{
    (void)pvParameters;

    powertrain_ctx_t *ctx = powertrain_ctx();
    if (ctx == NULL) {
        sim_trace_u32("pt_fatal", 1);
        vTaskSuspend(NULL);
        return;
    }

    powertrain_init(ctx);
    powertrain_primitives_init(ctx);
    ctx->task_handle = xTaskGetCurrentTaskHandle();
    if (ctx->diag_force_service_mode) {
        torque_controller_set_mode(&ctx->tc, VEHICLE_SERVICE);
        torque_controller_set_input(&ctx->tc, 80, 0, 0);
    }
    if (ctx->charging_force_mode) {
        torque_controller_set_mode(&ctx->tc, VEHICLE_CHARGING);
        torque_controller_set_input(&ctx->tc, 80, 0, 0);
    }

    // Create subordinate tasks.
    xTaskCreate(sensor_poll, "sensor", 512, NULL, 3, NULL);
    xTaskCreate(logger, "logger", 512, NULL, 1, NULL);
    xTaskCreate(deadline_monitor, "dl_mon", 384, NULL, 2, NULL);

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;

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
            mc_can_frame_t rx;
            uint32_t dlc = sim_can_recv(0, rx.data, MC_MAX_PAYLOAD_SIZE,
                                        &can_id, &is_ext, &is_remote);
            if (dlc == 0) break;

            rx.id = can_id;
            rx.sender = rx.data[0];
            rx.len = (uint8_t)dlc;
            dispatch_frame(ctx, &rx);
        }
        if (ctx->diag_force_service_mode) {
            torque_controller_set_mode(&ctx->tc, VEHICLE_SERVICE);
            torque_controller_set_input(&ctx->tc, 80, 0, 0);
        }
        if (ctx->charging_force_mode) {
            torque_controller_set_mode(&ctx->tc, VEHICLE_CHARGING);
            torque_controller_set_input(&ctx->tc, 80, 0, 0);
        }

        // ── Compute torque ────────────────────────────────────
        int8_t torque = torque_controller_compute(&ctx->tc);

        // SEEDED DEBUG-GYM BUG (service_torque): the SERVICE-mode safety clamp
        // is skipped, so a service session still commands drive torque with the
        // motor enabled. The fixed firmware (powertrain_diag_service) clamps
        // torque to 0 and disables the motor. Off by default.
        if (ctx->diag_service_clamp_bug && ctx->tc.vehicle_mode == VEHICLE_SERVICE) {
            torque = (int8_t)ctx->tc.throttle_percent;
            ctx->tc.motor_enable = 1;
        }

        // ── Send motor command (with semaphore guard) ────────
        send_motor_command(ctx, torque, &tx);
        if (ctx->tc.vehicle_mode == VEHICLE_SERVICE) {
            sim_trace_u32("diag_motor_command",
                          ((uint32_t)(uint8_t)torque << 8)
                        | (uint32_t)ctx->tc.motor_enable);
        }
        if (ctx->tc.vehicle_mode == VEHICLE_CHARGING) {
            sim_trace_u32("charging_motor_command",
                          ((uint32_t)(uint8_t)torque << 8)
                        | (uint32_t)ctx->tc.motor_enable);
        }
        can_tx_with_semaphore(ctx, &tx);

        // ── Send heartbeat ────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            can_tx_with_semaphore(ctx, &tx);
        }
    }
}
