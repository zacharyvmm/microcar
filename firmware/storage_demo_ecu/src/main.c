// main.c — storage demo ECU firmware
//
// Exercises the FlatMemoryStore (virtual block device) API:
//   - Creates a block device (256-byte pages, 16 pages, erase 0xFF)
//   - Writes "HELLO_BLOCK_WRITE_TEST" at offset 0
//   - Reads back and verifies the data
//   - Erases page 0, verifies all bytes are 0xFF
//   - Traces all operations via sim_trace_u32

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"
#include <string.h>

// ── Test data ──────────────────────────────────────────────────────────────

static const char g_test_data[] = "HELLO_BLOCK_WRITE_TEST";
#define TEST_DATA_LEN (sizeof(g_test_data) - 1)  // exclude null terminator

// ── Block device parameters ────────────────────────────────────────────────

#define BLOCK_ID          0
#define BLOCK_PAGE_SIZE   256
#define BLOCK_PAGE_COUNT  16
#define BLOCK_ERASE_VALUE 0xFF

// ── Task: storage demo ─────────────────────────────────────────────────────

void storage_demo_main(void *pvParameters)
{
    (void)pvParameters;

    sim_trace_u32("storage_demo_start", 1);

    // 1. Create the block device
    uint32_t result = sim_block_create(BLOCK_ID, BLOCK_PAGE_SIZE,
                                       BLOCK_PAGE_COUNT, BLOCK_ERASE_VALUE);
    sim_trace_u32("block_create_result", result);

    // 2. Get geometry and trace it
    uint32_t page_size = 0;
    uint32_t page_count = 0;
    sim_block_get_geometry(BLOCK_ID, &page_size, &page_count);
    sim_trace_u32("block_page_size", page_size);
    sim_trace_u32("block_page_count", page_count);

    // 3. Write test data at offset 0
    uint32_t write_result = sim_block_write(BLOCK_ID, 0,
                                            (const uint8_t *)g_test_data,
                                            TEST_DATA_LEN);
    sim_trace_u32("write_result", write_result);

    // 4. Read back and verify
    uint8_t read_buf[64] = {0};
    uint32_t read_result = sim_block_read(BLOCK_ID, 0, read_buf, TEST_DATA_LEN);
    sim_trace_u32("read_result", read_result);

    if (memcmp(read_buf, g_test_data, TEST_DATA_LEN) == 0) {
        sim_trace_u32("verify_MATCH", 1);
    } else {
        sim_trace_u32("verify_MISMATCH", 0);
        sim_trace_u32("read_result_code", read_result);
    }

    // 5. Erase page 0 and verify all bytes are 0xFF
    sim_block_erase_page(BLOCK_ID, 0);
    sim_trace_u32("erase_page_0", 1);

    // Read back the erased page
    uint8_t erase_check[BLOCK_PAGE_SIZE];
    memset(erase_check, 0x00, sizeof(erase_check)); // fill with non-0xFF to detect real read
    uint32_t erase_read_result = sim_block_read(BLOCK_ID, 0, erase_check, BLOCK_PAGE_SIZE);
    sim_trace_u32("erase_read_result", erase_read_result);

    // Verify all bytes are 0xFF
    int all_erased = 1;
    for (uint32_t i = 0; i < BLOCK_PAGE_SIZE; i++) {
        if (erase_check[i] != BLOCK_ERASE_VALUE) {
            all_erased = 0;
            break;
        }
    }

    sim_trace_u32("erase_verify_all_0xFF", all_erased ? 1 : 0);

    sim_trace_u32("storage_demo_complete", 1);

    // Loop with delay
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(100));
    }
}
