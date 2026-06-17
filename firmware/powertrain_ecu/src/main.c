// main.c — powertrain ECU firmware
//
// The powertrain controls the motor. It:
// 1. Receives driver input from the plant
// 2. Receives vehicle mode from the gateway
// 3. Receives BMS torque limits
// 4. Computes safe motor torque
// 5. Watches for gateway heartbeat timeout (S4)
//
// Compiles as a FreeRTOS task running on the costar simulator.

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

// ── Boot ──────────────────────────────────────────────────────────────────

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

// ── Main loop ─────────────────────────────────────────────────────────────

void powertrain_main(void)
{
    powertrain_init();

    uint32_t uptime_ms = 0;
    mc_can_frame_t tx;

    send_heartbeat(0, &tx);
    // tx → CAN controller

    while (1) {
        // vTaskDelay(pdMS_TO_TICKS(10));
        uptime_ms += 10;

        // ── Watchdog check ────────────────────────────────────
        uint8_t timeout = watchdog_check(&g_wd, uptime_ms);
        if (timeout) {
            // S4: Gateway heartbeat lost → disable torque.
            g_tc.motor_enable = 0;
        }

        // ── Compute torque ────────────────────────────────────
        int8_t torque = torque_controller_compute(&g_tc);

        // ── Send motor command ────────────────────────────────
        send_motor_command(torque, &tx);
        // sim_can_send(&tx);

        // ── Send heartbeat ────────────────────────────────────
        if (uptime_ms % 100 == 0) {
            send_heartbeat(uptime_ms, &tx);
            // sim_can_send(&tx);
        }
    }
}
