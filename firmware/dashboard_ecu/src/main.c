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
#include "microcar_dashboard.h"
#include "microcar_ota_slot.h"
#include <stdalign.h>

// ── Instance key — allocated once per machine via sim_instance_state ──────

#define DASH_KEY 0x4D430004

// ── Per-instance context (replaces all file-scope statics) ────────────────

typedef struct {
    dashboard_state_t   ds;
    warning_display_t   wd;
    mc_dash_state_t     dash;
    uint8_t             display_initialized;
    uint16_t            framebuffer[MC_DASH_WIDTH * MC_DASH_HEIGHT];

    // Task handle for display_update (needed for xTaskNotify).
    TaskHandle_t        display_task_handle;
} dashboard_ctx_t;

/// Return the per-machine dashboard context, allocating it on first call.
static dashboard_ctx_t *dashboard_ctx(void)
{
    dashboard_ctx_t *ctx = (dashboard_ctx_t *)sim_instance_state(
        DASH_KEY, sizeof(dashboard_ctx_t), alignof(dashboard_ctx_t));
    return ctx;
}

void dashboard_init(dashboard_ctx_t *ctx)
{
    dashboard_state_init(&ctx->ds);
    warning_display_init(&ctx->wd);
    mc_dash_init(&ctx->dash);
    ctx->display_initialized = 0;
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process vehicle mode frame (0x010) from gateway.
static void handle_vehicle_mode(dashboard_ctx_t *ctx,
                                const mc_can_frame_t *frame)
{
    uint8_t mode = frame->data[0];
    dashboard_state_set_mode(&ctx->ds, (mc_vehicle_mode_t)mode);
    mc_dash_set_mode(&ctx->dash, (mc_vehicle_mode_t)mode);
}

/// Process wheel speed frame (0x102) from plant.
static void handle_wheel_speed(dashboard_ctx_t *ctx,
                               const mc_can_frame_t *frame)
{
    uint16_t speed_kph_x10 = ((uint16_t)frame->data[0] << 8) | frame->data[1];
    ctx->dash.speed_kmh = (uint8_t)(speed_kph_x10 / 10);
    dashboard_state_set_speed(&ctx->ds, speed_kph_x10);
}

/// Process plant sensor data frame (0x500).
/// Format: [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo]
static void handle_plant_sensors(dashboard_ctx_t *ctx,
                                 const mc_can_frame_t *frame)
{
    if (frame->len < 7) return;

    uint8_t  soc_percent  = frame->data[0];
    uint16_t voltage_mv   = ((uint16_t)frame->data[1] << 8) | frame->data[2];
    int16_t  temp_c_x10   = (int16_t)(((uint16_t)frame->data[3] << 8) | frame->data[4]);
    uint16_t current_raw  = ((uint16_t)frame->data[5] << 8) | frame->data[6];

    dashboard_state_set_battery(&ctx->ds, soc_percent, temp_c_x10, voltage_mv);
    ctx->dash.soc_percent  = soc_percent;
    ctx->dash.current_a_x2 = (uint8_t)(current_raw / 500);
}

/// Process warning frame (0x400).
static void handle_warning(dashboard_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t source_node  = frame->data[0];
    uint8_t warning_code = frame->data[1];

    dashboard_state_add_warning(&ctx->ds, warning_code, source_node);

    uint8_t severity = warning_display_severity_for(warning_code);
    warning_display_update(&ctx->wd, warning_code, severity);

    // Notify the display_update task of a new/updated warning.
    // The notification value carries the warning code + severity.
    if (ctx->display_task_handle != NULL) {
        uint32_t notify_val = ((uint32_t)severity << 8) | warning_code;
        xTaskNotify(ctx->display_task_handle, notify_val, eSetValueWithoutOverwrite);
    }
}

/// Process OTA status frame (0x633) from gateway.
static void handle_ota_status(dashboard_ctx_t *ctx,
                              const mc_can_frame_t *frame)
{
    if (frame->len < 8) return;
    uint8_t ota_state = frame->data[2];
    switch (ota_state) {
    case MC_OTA_IDLE:           ctx->dash.ota_progress = 0;   break;
    case MC_OTA_DOWNLOADING:    ctx->dash.ota_progress = 25;  break;
    case MC_OTA_VERIFYING:      ctx->dash.ota_progress = 50;  break;
    case MC_OTA_COMMIT_PENDING: ctx->dash.ota_progress = 75;  break;
    case MC_OTA_REBOOTING:      ctx->dash.ota_progress = 90;  break;
    case MC_OTA_HEALTHY:        ctx->dash.ota_progress = 100; break;
    case MC_OTA_ROLLED_BACK:    ctx->dash.ota_progress = 0;   break;
    default:                    ctx->dash.ota_progress = 0;   break;
    }
}

/// Process motor command frame (0x101) from gateway — extract torque.
static void handle_motor_command(dashboard_ctx_t *ctx,
                                 const mc_can_frame_t *frame)
{
    ctx->dash.torque_percent = (int8_t)frame->data[0];
}

/// Dispatch a received CAN frame to the appropriate handler.
static void dispatch_frame(dashboard_ctx_t *ctx,
                           const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_VEHICLE_MODE:
        handle_vehicle_mode(ctx, frame);
        break;
    case MC_MSG_WHEEL_SPEED:
        handle_wheel_speed(ctx, frame);
        break;
    case MC_MSG_PLANT_SENSORS:
        handle_plant_sensors(ctx, frame);
        break;
    case MC_MSG_WARNING:
        handle_warning(ctx, frame);
        break;
    case MC_MSG_OTA_STATUS:
        handle_ota_status(ctx, frame);
        break;
    case MC_MSG_MOTOR_COMMAND:
        handle_motor_command(ctx, frame);
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

/// Background task that processes display rendering and touch input.
/// Runs at 50 ms period and updates the virtual display.
void display_update(void *pvParameters)
{
    dashboard_ctx_t *ctx = (dashboard_ctx_t *)pvParameters;

    uint32_t prev_notify_val = 0;
    uint32_t last_render_ms  = 0;

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

        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // ── One-time display initialisation ───────────────────────
        if (!ctx->display_initialized) {
            sim_display_init(0, MC_DASH_WIDTH, MC_DASH_HEIGHT, 0);
            sim_touch_init(0, 0);
            sim_display_enable(0, 1);
            sim_display_set_backlight(0, 80);
            ctx->display_initialized = 1;
            sim_trace_u32("display_init", 1);
        }

        // ── Touch polling (page toggle) ───────────────────────────
        {
            uint32_t point_id;
            uint16_t tx, ty;
            uint8_t  ev_pressure;
            uint32_t ev_type;
            while (sim_touch_get_event(0, &point_id, &tx, &ty,
                                       &ev_pressure, &ev_type) == 1) {
                if (ev_type == 1 && tx >= 280 && tx <= 319 && ty <= 39) {
                    ctx->dash.page = ctx->dash.page ? 0 : 1;
                    sim_trace_u32("dash_page", ctx->dash.page);
                }
            }
        }

        // ── Render every 100 ms ───────────────────────────────────
        if (now_ms - last_render_ms >= 100) {
            mc_dash_render(&ctx->dash, ctx->framebuffer);
            sim_display_draw_bitmap(0, 0, 0, MC_DASH_WIDTH, MC_DASH_HEIGHT,
                                    (const uint8_t *)ctx->framebuffer,
                                    MC_DASH_WIDTH * MC_DASH_HEIGHT * 2);
            last_render_ms = now_ms;
        }

        // Periodic trace every 500ms.
        if (now_ms % 500 == 0) {
            uint8_t top = dashboard_state_top_warning(&ctx->ds);
            sim_trace_u32("display_update", top);
        }
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

void dashboard_main(void *pvParameters)
{
    (void)pvParameters;

    dashboard_ctx_t *ctx = dashboard_ctx();
    dashboard_init(ctx);

    // Create the display_update task (prio 2, lower freq).
    // Pass the context pointer so display_update can access state.
    xTaskCreate(display_update, "display", 512,
                ctx, 2, &ctx->display_task_handle);

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

        // ── Check for top warning ──────────────────────────────
        uint8_t top_warning = dashboard_state_top_warning(&ctx->ds);
        (void)top_warning;

        // ── Send heartbeat ────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
