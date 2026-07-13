// main.c — diagnostics tool firmware
//
// A small service-tool ECU for the dogfood diagnostics lane. It sends a fixed
// request script and traces gateway responses. Scenarios can inject faults or
// driver input around the script to prove SERVICE mode and DTC behavior.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

#include "microcar_protocol.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>

#define CAN_BUS 0

static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_DIAGNOSTICS,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_DIAGNOSTICS;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

static void send_diag_request(uint8_t service, uint8_t request_id, uint8_t param,
                              mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_DIAG_REQUEST, MC_NODE_DIAGNOSTICS,
                  MC_DIAG_REQUEST_MSG_SIZE);
    tx->data[0] = MC_NODE_DIAGNOSTICS;
    tx->data[1] = service;
    tx->data[2] = request_id;
    tx->data[3] = param;
    sim_can_send(CAN_BUS, tx->id, tx->data, tx->len, 0, 0);
}

static void handle_diag_response(const mc_can_frame_t *frame)
{
    if (frame->len < MC_DIAG_RESPONSE_MSG_SIZE) return;

    uint8_t service = frame->data[1];
    uint8_t request_id = frame->data[2];
    uint8_t status = frame->data[3];
    uint8_t value0 = frame->data[4];
    uint8_t value1 = frame->data[5];

    uint32_t packed = ((uint32_t)request_id << 24)
                    | ((uint32_t)service << 16)
                    | ((uint32_t)status << 8)
                    | (uint32_t)value0;
    sim_trace_u32("diag_response", packed);

    switch (service) {
    case MC_DIAG_START_SESSION:
        sim_trace_u32("diag_session_rsp", ((uint32_t)status << 8) | value0);
        break;
    case MC_DIAG_READ_MODE:
        sim_trace_u32("diag_mode_rsp", ((uint32_t)status << 8) | value0);
        break;
    case MC_DIAG_READ_DTCS:
        sim_trace_u32("diag_dtcs_rsp",
                      ((uint32_t)request_id << 16)
                    | ((uint32_t)value0 << 8)
                    | (uint32_t)value1);
        break;
    case MC_DIAG_CLEAR_DTCS:
        sim_trace_u32("diag_clear_rsp", (uint32_t)status);
        break;
    case MC_DIAG_ACTUATOR_TEST:
        sim_trace_u32("diag_actuator_rsp", ((uint32_t)status << 8) | value0);
        break;
    default:
        break;
    }
}

void diagnostics_tool_main(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;
    uint32_t sent_mask = 0;
    uint32_t script_ms = 0;

    send_heartbeat(0, &tx);
    sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        script_ms += 10;

        if (script_ms >= 100 && !(sent_mask & 0x01)) {
            send_diag_request(MC_DIAG_START_SESSION, 1, 0, &tx);
            sent_mask |= 0x01;
        }
        if (script_ms >= 200 && !(sent_mask & 0x02)) {
            send_diag_request(MC_DIAG_READ_MODE, 2, 0, &tx);
            sent_mask |= 0x02;
        }
        if (script_ms >= 350 && !(sent_mask & 0x04)) {
            send_diag_request(MC_DIAG_ACTUATOR_TEST, 6, 0, &tx);
            sent_mask |= 0x04;
        }
        if (script_ms >= 450 && !(sent_mask & 0x08)) {
            send_diag_request(MC_DIAG_READ_DTCS, 3, 0, &tx);
            sent_mask |= 0x08;
        }
        if (script_ms >= 650 && !(sent_mask & 0x10)) {
            send_diag_request(MC_DIAG_CLEAR_DTCS, 4, 0, &tx);
            sent_mask |= 0x10;
        }
        if (script_ms >= 750 && !(sent_mask & 0x20)) {
            send_diag_request(MC_DIAG_READ_DTCS, 5, 0, &tx);
            sent_mask |= 0x20;
        }

        uint32_t can_id;
        uint32_t is_ext;
        uint32_t is_remote;
        while (1) {
            mc_can_frame_t rx;
            uint32_t dlc = sim_can_recv(CAN_BUS, rx.data, MC_MAX_PAYLOAD_SIZE,
                                        &can_id, &is_ext, &is_remote);
            if (dlc == 0) break;

            rx.id = can_id;
            rx.sender = rx.data[0];
            rx.len = (uint8_t)dlc;
            if (rx.id == MC_MSG_DIAG_RESPONSE && rx.sender == MC_NODE_GATEWAY) {
                handle_diag_response(&rx);
            }
        }

        if (script_ms % 100 == 0) {
            send_heartbeat(script_ms, &tx);
            sim_can_send(CAN_BUS, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
