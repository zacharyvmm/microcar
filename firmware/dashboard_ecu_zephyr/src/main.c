// main.c — Zephyr-based dashboard ECU firmware
//
// The dashboard displays vehicle state to the driver. It:
// 1. Receives vehicle mode from the gateway
// 2. Receives wheel speed from the powertrain
// 3. Receives battery state from BMS
// 4. Displays warning messages based on severity
// 5. Sends periodic heartbeats
//
// Uses Zephyr threading APIs (k_thread, k_sleep) instead of FreeRTOS.
// Compiles via costar's sim-zephyr-port cc crate.
//
// Entry point is zephyr_app_main() — Zephyr's init.c has main→zephyr_app_main
// so the kernel calls this after boot.

#include "dashboard_state.h"
#include "warning_display.h"
#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <zephyr/kernel.h>
#include <string.h>

#define CAN_BUS 0

// ── ABI helpers (from costar sim-ffi) ─────────────────────────────────────
// When compiled with the full Zephyr kernel, sim_abi.h is available.
// When compiled standalone, trace calls are no-ops.
#ifdef SIMULATION_HOST_MODE
#include "sim_abi.h"
#else
#define sim_trace_u32(label, val) ((void)0)
#endif

// ── Global state ──────────────────────────────────────────────────────────

static dashboard_state_t g_ds;
static warning_display_t g_wd;

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
    sim_trace_u32("dash_mode", mode);
}

/// Process wheel speed frame (0x200) from plant.
static void handle_wheel_speed(const mc_can_frame_t *frame)
{
    uint16_t speed_kph_x10 = ((uint16_t)frame->data[0] << 8) | frame->data[1];
    dashboard_state_set_speed(&g_ds, speed_kph_x10);
    sim_trace_u32("dash_speed", speed_kph_x10);
}

/// Process BMS status frame (0x300) from plant.
/// Format: [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo]
static void handle_bms_status(const mc_can_frame_t *frame)
{
    if (frame->len < 7) return;

    uint8_t  soc_percent = frame->data[0];
    uint16_t voltage_mv  = ((uint16_t)frame->data[1] << 8) | frame->data[2];
    int16_t  temp_c_x10  = (int16_t)(((uint16_t)frame->data[3] << 8) | frame->data[4]);

    dashboard_state_set_battery(&g_ds, soc_percent, temp_c_x10, voltage_mv);
    sim_trace_u32("dash_batt", soc_percent);
}

/// Process warning frame (0x400).
static void handle_warning(const mc_can_frame_t *frame)
{
    uint8_t source_node  = frame->data[0];
    uint8_t warning_code = frame->data[1];

    dashboard_state_add_warning(&g_ds, warning_code, source_node);

    uint8_t severity = warning_display_severity_for(warning_code);
    warning_display_update(&g_wd, warning_code, severity);
    sim_trace_u32("dash_warn", warning_code);
}

// ── CAN frame construction ────────────────────────────────────────────────

static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    tx->id     = MC_MSG_HEARTBEAT;
    tx->sender = MC_NODE_DASHBOARD;
    tx->len    = MC_HEARTBEAT_MSG_SIZE;
    tx->data[0] = MC_NODE_DASHBOARD;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

// ── Zephyr thread definitions ─────────────────────────────────────────────

#define DASHBOARD_STACK_SIZE 4096
#define DASHBOARD_PRIORITY 4

static K_THREAD_STACK_DEFINE(dashboard_stack, DASHBOARD_STACK_SIZE);
static struct k_thread dashboard_thread_data;

// ── Dashboard thread entry ────────────────────────────────────────────────

static void dashboard_thread_entry(void *arg1, void *arg2, void *arg3)
{
    (void)arg1; (void)arg2; (void)arg3;

    dashboard_init();
    sim_trace_u32("dash_boot", 1);

    uint32_t uptime_ms = 0;
    mc_can_frame_t tx;

    send_heartbeat(0, &tx);
    sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        k_sleep(K_MSEC(10));
        uptime_ms += 10;

        // ── Check for top warning ──────────────────────────────
        uint8_t top_warning = dashboard_state_top_warning(&g_ds);
        if (top_warning != MC_WARN_NONE) {
            sim_trace_u32("dash_top_warn", top_warning);
        }

        // ── Send heartbeat ────────────────────────────────────
        if (uptime_ms % 100 == 0) {
            send_heartbeat(uptime_ms, &tx);
            sim_trace_u32("dash_hb", uptime_ms);
            sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
        }

        // ── Publish dashboard status ──────────────────────────
        // On state changes, send dashboard_status (0x300) or
        // warning frame (0x400) if active warning changed.
    }
}

// ── Zephyr app entry point ────────────────────────────────────────────────

int zephyr_app_main(void)
{
    sim_trace_u32("dash_zephyr_main", 1);

    k_thread_create(&dashboard_thread_data,
                    dashboard_stack,
                    K_THREAD_STACK_SIZEOF(dashboard_stack),
                    dashboard_thread_entry,
                    NULL, NULL, NULL,
                    DASHBOARD_PRIORITY, 0, K_NO_WAIT);

    sim_trace_u32("dash_main_done", 1);

    // Block forever — Zephyr threads should not return.
    k_sleep(K_FOREVER);
    return 0;
}
