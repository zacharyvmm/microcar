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

#include "FreeRTOS.h"
#include "task.h"
#include "queue.h"
#include "semphr.h"
#include "timers.h"
#include "sim_abi.h"

#define CAN_BUS 0

#include "torque_controller.h"
#include "safety_rules.h"
#include "watchdog_task.h"
#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>

// ── Global state ──────────────────────────────────────────────────────────

static torque_controller_t g_tc;
static watchdog_task_t     g_wd;

// ── Counting semaphore (CAN TX mailbox) ───────────────────────────────────

/// Counting semaphore representing available CAN TX mailbox slots.
/// Initialised to 3 (max 3 pending TX frames).
static SemaphoreHandle_t g_can_tx_slots = NULL;

// ── Software timer (watchdog check) ───────────────────────────────────────

/// Software timer that fires every 50ms to call watchdog_check.
/// Uses FreeRTOS software timer (xTimerCreate).
static TimerHandle_t g_wd_timer = NULL;

// ── Task handles ──────────────────────────────────────────────────────────

static TaskHandle_t g_powertrain_task_handle = NULL;

// ── Watchdog timer callback ───────────────────────────────────────────────

/// Software timer callback: check gateway watchdog and trace status.
static void watchdog_timer_cb(TimerHandle_t xTimer)
{
    (void)xTimer;
    uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
    uint8_t timeout = watchdog_check(&g_wd, now_ms);

    if (timeout) {
        // S4: Gateway heartbeat lost → disable torque.
        g_tc.motor_enable = 0;
        sim_trace_u32("gateway_timeout", now_ms);
    } else {
        sim_trace_u32("watchdog_ok", now_ms);
    }
}

// ── Boot ──────────────────────────────────────────────────────────────────

/// Allocate FreeRTOS primitives. Called once from powertrain_main.
static void powertrain_primitives_init(void)
{
    g_can_tx_slots = xSemaphoreCreateCounting(3, 3);

    g_wd_timer = xTimerCreate(
        "wd_timer",
        pdMS_TO_TICKS(50),
        pdTRUE,   // auto-reload
        NULL,
        watchdog_timer_cb);

    if (g_wd_timer != NULL) {
        xTimerStart(g_wd_timer, 0);
    }

    sim_trace_u32("pt_can_sem", g_can_tx_slots != NULL ? 1 : 0);
    sim_trace_u32("pt_wd_timer", g_wd_timer != NULL ? 1 : 0);
}

void powertrain_init(void)
{
    torque_controller_init(&g_tc);
    watchdog_init(&g_wd);
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a driver input frame (0x020) from the plant.
static void handle_driver_input(const mc_can_frame_t *frame)
{
    uint8_t throttle = frame->data[0];
    uint8_t brake    = frame->data[1];
    uint8_t gear     = frame->len >= 3 ? frame->data[2] : 0;

    torque_controller_set_input(&g_tc, throttle, brake, gear);
}

/// Process a vehicle mode frame (0x010) from the gateway.
static void handle_vehicle_mode(const mc_can_frame_t *frame)
{
    uint8_t mode = frame->data[0];
    torque_controller_set_mode(&g_tc, (mc_vehicle_mode_t)mode);
}

/// Process a BMS limits frame (0x201).
static void handle_bms_limits(const mc_can_frame_t *frame)
{
    uint8_t max_torque = frame->data[0];
    torque_controller_set_bms_limit(&g_tc, max_torque);
}

/// Process a gateway heartbeat frame (0x001).
static void handle_gateway_heartbeat(uint32_t now_ms)
{
    watchdog_gateway_beat(&g_wd, now_ms);
}

/// Dispatch a received CAN frame to the appropriate handler.
static void dispatch_frame(const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_DRIVER_INPUT:
        handle_driver_input(frame);
        break;
    case MC_MSG_VEHICLE_MODE:
        handle_vehicle_mode(frame);
        break;
    case MC_MSG_BMS_LIMITS:
        handle_bms_limits(frame);
        break;
    case MC_MSG_HEARTBEAT:
        if (frame->sender == MC_NODE_GATEWAY) {
            uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
            handle_gateway_heartbeat(now_ms);
        }
        break;
    default:
        break;
    }
}

// ── CAN TX with counting semaphore ────────────────────────────────────────

/// Send a CAN frame using the counting semaphore as mailbox.
/// Takes a slot, sends, then gives it back after a short delay.
static void can_tx_with_semaphore(mc_can_frame_t *frame)
{
    if (xSemaphoreTake(g_can_tx_slots, pdMS_TO_TICKS(2)) == pdTRUE) {
        sim_can_send(0, frame->id, frame->data, frame->len, 0, 0);
        // Release the mailbox slot (simulates TX completion interrupt).
        xSemaphoreGive(g_can_tx_slots);
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

static void send_motor_command(int8_t torque, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_MOTOR_COMMAND, MC_NODE_POWERTRAIN,
                  MC_MOTOR_COMMAND_MSG_SIZE);
    tx->data[0] = (uint8_t)torque;
    tx->data[1] = g_tc.motor_enable;
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
/// This makes scheduler performance regressions immediately visible in trace
/// output — if a costar change introduces timing drift, golden trace
/// comparison catches it.
void deadline_monitor(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(50));

        TickType_t now = xTaskGetTickCount();
        TickType_t expected = last_wake;
        // last_wake was updated by vTaskDelayUntil to the target wake time.
        // actual wake time is 'now'. Jitter = |now - expected|.
        // Note: vTaskDelayUntil advances last_wake, so expected = last_wake
        // after the call minus the increment. We capture expected before
        // vTaskDelayUntil modifies it.

        // Compute jitter (signed, then abs)
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
/// Traces torque commands and gateway status.
void logger(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // Log current torque and gateway online status.
        uint8_t gw_online = watchdog_gateway_online(&g_wd);
        uint32_t log_val = ((uint32_t)gw_online << 16) | (g_tc.motor_enable & 1);
        sim_trace_u32("logger_event", log_val);

        (void)now_ms;
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

void powertrain_main(void *pvParameters)
{
    (void)pvParameters;
    powertrain_init();
    powertrain_primitives_init();
    g_powertrain_task_handle = xTaskGetCurrentTaskHandle();

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
            dispatch_frame(&rx);
        }

        // ── Compute torque ────────────────────────────────────
        int8_t torque = torque_controller_compute(&g_tc);

        // ── Send motor command (with semaphore guard) ────────
        send_motor_command(torque, &tx);
        can_tx_with_semaphore(&tx);

        // ── Send heartbeat ────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            can_tx_with_semaphore(&tx);
        }
    }
}
