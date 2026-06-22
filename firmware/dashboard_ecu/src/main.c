// main.c — dashboard ECU firmware
//
// The dashboard displays vehicle state to the driver. It:
// 1. Receives vehicle mode from the gateway
// 2. Receives wheel speed from the plant/powertrain
// 3. Receives battery state from BMS/plant
// 4. Displays warning messages based on severity
// 5. Sends periodic heartbeats
//
// Multi-task: dashboard_main (prio 1) handles CAN I/O,
// display_update (prio 2) processes warning notifications.
//
// Compiles as FreeRTOS tasks running on the costar simulator.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

#define CAN_BUS 0

#include "dashboard_state.h"
#include "warning_display.h"
#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>

// ── Global state ──────────────────────────────────────────────────────────

static dashboard_state_t g_ds;
static warning_display_t g_wd;

// ── Task notification ─────────────────────────────────────────────────────

/// Task handle for display_update (needed for xTaskNotify).
static TaskHandle_t g_display_task_handle = NULL;

// ── Boot ──────────────────────────────────────────────────────────────────

void dashboard_init(void)
{
    dashboard_state_init(&g_ds);
    warning_display_init(&g_wd);
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process vehicle mode frame (0x010) from gateway.
static void handle_vehicle_mode(const mc_can_frame_t *frame)
{
    uint8_t mode = frame->data[0];
    dashboard_state_set_mode(&g_ds, (mc_vehicle_mode_t)mode);
}

/// Process wheel speed frame (0x102) from plant.
static void handle_wheel_speed(const mc_can_frame_t *frame)
{
    uint16_t speed_kph_x10 = ((uint16_t)frame->data[0] << 8) | frame->data[1];
    dashboard_state_set_speed(&g_ds, speed_kph_x10);
}

/// Process plant sensor data frame (0x500).
/// Format: [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo]
static void handle_plant_sensors(const mc_can_frame_t *frame)
{
    if (frame->len < 7) return;

    uint8_t  soc_percent = frame->data[0];
    uint16_t voltage_mv  = ((uint16_t)frame->data[1] << 8) | frame->data[2];
    int16_t  temp_c_x10  = (int16_t)(((uint16_t)frame->data[3] << 8) | frame->data[4]);

    dashboard_state_set_battery(&g_ds, soc_percent, temp_c_x10, voltage_mv);
}

/// Process warning frame (0x400).
static void handle_warning(const mc_can_frame_t *frame)
{
    uint8_t source_node  = frame->data[0];
    uint8_t warning_code = frame->data[1];

    dashboard_state_add_warning(&g_ds, warning_code, source_node);

    uint8_t severity = warning_display_severity_for(warning_code);
    warning_display_update(&g_wd, warning_code, severity);

    // Notify the display_update task of a new/updated warning.
    // The notification value carries the warning code + severity.
    if (g_display_task_handle != NULL) {
        uint32_t notify_val = ((uint32_t)severity << 8) | warning_code;
        xTaskNotify(g_display_task_handle, notify_val, eSetValueWithoutOverwrite);
    }
}

/// Dispatch a received CAN frame to the appropriate handler.
static void dispatch_frame(const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_VEHICLE_MODE:
        handle_vehicle_mode(frame);
        break;
    case MC_MSG_WHEEL_SPEED:
        handle_wheel_speed(frame);
        break;
    case MC_MSG_PLANT_SENSORS:
        handle_plant_sensors(frame);
        break;
    case MC_MSG_WARNING:
        handle_warning(frame);
        break;
    default:
        break;
    }
}

// ── CAN frame construction ────────────────────────────────────────────────

static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_DASHBOARD,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_DASHBOARD;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

// ── Display update task ────────────────────────────────────────────────────

/// Background task that processes warning notifications via xTaskNotifyWait.
/// Runs at lower frequency (50ms) and updates the display.
void display_update(void *pvParameters)
{
    (void)pvParameters;

    uint32_t prev_notify_val = 0;

    while (1) {
        // Wait for a notification or timeout at 50ms.
        uint32_t notify_val = 0;
        BaseType_t notified = xTaskNotifyWait(
            0x00000000,           // Don't clear any bits on entry
            0xFFFFFFFF,           // Clear all bits on exit
            &notify_val,
            pdMS_TO_TICKS(50));

        if (notified == pdTRUE && notify_val != prev_notify_val) {
            uint8_t warning_code = (uint8_t)(notify_val & 0xFF);
            uint8_t severity     = (uint8_t)(notify_val >> 8);
            sim_trace_u32("display_warning", notify_val);
            (void)severity;
            (void)warning_code;
            prev_notify_val = notify_val;
        }

        // Periodic refresh: emit display_update trace every 500ms.
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;
        if (now_ms % 500 == 0) {
            uint8_t top = dashboard_state_top_warning(&g_ds);
            sim_trace_u32("display_update", top);
        }
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

void dashboard_main(void *pvParameters)
{
    (void)pvParameters;
    dashboard_init();

    // Create the display_update task (prio 2, lower freq).
    xTaskCreate(display_update, "display", 512,
                NULL, 2, &g_display_task_handle);

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

        // ── Check for top warning ──────────────────────────────
        uint8_t top_warning = dashboard_state_top_warning(&g_ds);
        (void)top_warning;

        // ── Send heartbeat ────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
