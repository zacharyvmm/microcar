// main.c — net_demo_ecu firmware
//
// Exercises the VirtualEthDevice API with a simple ping-pong pattern.
// Registers an Ethernet device, sends a "HELLO_FROM_NET" frame every
// ~100ms, and polls for incoming frames.  All sends and receives are
// traced via sim_trace_u32.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include <string.h>

// ── Ethernet device configuration ────────────────────────────────────────

#define ETH_DEVICE_ID    0
#define ETH_MTU          1500
#define ETH_RX_BUF_SIZE  1522   // max Ethernet frame (1500 MTU + headers)

static const uint8_t ETH_MAC[6] = {
    0x02, 0x00, 0x00, 0x00, 0x00, 0x01
};

static const char HELLO_MSG[] = "HELLO_FROM_NET";
#define HELLO_MSG_LEN  (sizeof(HELLO_MSG) - 1)   // exclude null terminator

// ── Main task ────────────────────────────────────────────────────────────

void net_demo_main(void *pvParameters)
{
    (void)pvParameters;

    uint8_t       rx_buf[ETH_RX_BUF_SIZE];
    uint32_t      send_count = 0;
    uint32_t      recv_count = 0;

    // Register the virtual Ethernet device.
    uint32_t reg_result = sim_eth_register(ETH_DEVICE_ID, ETH_MAC, ETH_MTU);
    sim_trace_u32("eth_register", reg_result);
    sim_trace_u32("net_demo_boot", 1);

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        // ── Send a HELLO frame ─────────────────────────────────
        uint32_t sent = sim_eth_send(ETH_DEVICE_ID,
                                     (const uint8_t *)HELLO_MSG,
                                     HELLO_MSG_LEN);
        send_count++;
        sim_trace_u32("eth_send_count", send_count);
        sim_trace_u32("eth_send_bytes", sent);

        // ── Poll and drain received frames ────────────────────
        while (sim_eth_poll(ETH_DEVICE_ID)) {
            uint32_t rx_len = sim_eth_recv(ETH_DEVICE_ID,
                                           rx_buf,
                                           ETH_RX_BUF_SIZE);
            recv_count++;
            sim_trace_u32("eth_recv_count", recv_count);
            sim_trace_u32("eth_recv_bytes", rx_len);

            // Trace the first 4 bytes of the received frame as u32
            if (rx_len >= 4) {
                uint32_t first_word = ((uint32_t)rx_buf[0] << 24)
                                    | ((uint32_t)rx_buf[1] << 16)
                                    | ((uint32_t)rx_buf[2] << 8)
                                    | ((uint32_t)rx_buf[3]);
                sim_trace_u32("eth_recv_first4", first_word);
            }
        }
    }
}
