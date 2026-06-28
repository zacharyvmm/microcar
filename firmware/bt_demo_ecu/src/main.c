// main.c — BT Demo ECU firmware
//
// Exercises the VirtualHciController API through the sim ABI:
//   - Registers HCI controller with ID 0
//   - Sends an HCI Reset command (opcode 0x0C03)
//   - Polls for HCI events via sim_bt_recv
//   - Receives the CommandComplete event for HCI Reset
//   - Traces all operations via sim_trace_u32

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include <string.h>

/* HCI packet types per Bluetooth Core Specification */
#define HCI_PKT_COMMAND  1
#define HCI_PKT_ACL      2
#define HCI_PKT_EVENT    4

/* HCI event codes */
#define HCI_EVT_COMMAND_COMPLETE  0x0E

void bt_demo_main(void *pvParameters)
{
    (void)pvParameters;
    sim_trace_u32("bt_demo_started", 1);

    /* ── Register virtual HCI controller 0 ─────────────────────── */
    uint32_t ctrl_id = sim_bt_register(0);
    sim_trace_u32("bt_register_id", ctrl_id);

    /* ── Send HCI Reset command ─────────────────────────────────── */
    /*
     * HCI Reset command format:
     *   [opcode_lo, opcode_hi, param_len]
     *   opcode = 0x0C03 (OGF=0x03 Link Control, OCF=0x0003 Reset)
     *   param_len = 0 (no parameters)
     */
    uint8_t hci_reset_cmd[] = { 0x03, 0x0C, 0x00 };
    sim_bt_send(ctrl_id, HCI_PKT_COMMAND, hci_reset_cmd, sizeof(hci_reset_cmd));
    sim_trace_u32("bt_cmd_sent_bytes", sizeof(hci_reset_cmd));

    /* ── Wait one tick for the controller to process ────────────── */
    vTaskDelay(1);
    sim_trace_u32("bt_polling_events", 1);

    /* ── Poll for HCI events ────────────────────────────────────── */
    uint8_t pkt_type = 0;
    uint8_t buf[64];
    uint32_t total_recv = 0;

    while (1) {
        uint32_t n = sim_bt_recv(ctrl_id, &pkt_type, buf, sizeof(buf));
        if (n == 0) {
            break;  /* no more events */
        }

        total_recv += n;
        sim_trace_u32("bt_recv_bytes", n);
        sim_trace_u32("bt_recv_pkt_type", pkt_type);

        if (pkt_type == HCI_PKT_EVENT) {
            uint8_t evt_code = buf[0];
            sim_trace_u32("bt_evt_code", evt_code);

            if (evt_code == HCI_EVT_COMMAND_COMPLETE) {
                sim_trace_u32("bt_CC_event", 1);
                /* buf[1]=param_len, buf[2]=num_hci_cmd_pkts,
                 * buf[3]=opcode_lo, buf[4]=opcode_hi, buf[5]=status */
                sim_trace_u32("bt_CC_param_len", buf[1]);
                sim_trace_u32("bt_CC_num_pkts", buf[2]);
                uint16_t cc_opcode = ((uint16_t)buf[4] << 8) | buf[3];
                sim_trace_u32("bt_CC_opcode", cc_opcode);
                sim_trace_u32("bt_CC_status", buf[5]);
            } else {
                sim_trace_u32("bt_unexpected_evt_code", evt_code);
            }
        } else if (pkt_type == HCI_PKT_ACL) {
            sim_trace_u32("bt_ACL_data", n);
        } else {
            sim_trace_u32("bt_unexpected_pkt_type", pkt_type);
        }
    }

    sim_trace_u32("bt_total_recv", total_recv);
    sim_trace_u32("bt_demo_finished", 1);

    /* idle loop */
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(1000));
    }
}
