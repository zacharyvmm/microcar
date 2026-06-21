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

#include "FreeRTOS.h"
#include "task.h"
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

void powertrain_main(void *pvParameters)
{
    (void)pvParameters;
    powertrain_init();
    sim_register_symbol((uint64_t)xTaskGetCurrentTaskHandle(), "powertrain_main");

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

        // ── Watchdog check ────────────────────────────────────
        uint8_t timeout = watchdog_check(&g_wd, now_ms);
        if (timeout) {
            // S4: Gateway heartbeat lost → disable torque.
            g_tc.motor_enable = 0;
        }

        // ── Compute torque ────────────────────────────────────
        int8_t torque = torque_controller_compute(&g_tc);

        // ── Send motor command ────────────────────────────────
        send_motor_command(torque, &tx);
        sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);

        // ── Send heartbeat ────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
