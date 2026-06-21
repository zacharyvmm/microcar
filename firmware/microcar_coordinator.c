// microcar_coordinator.c — Boot entry point for microcar firmware
//
// This file creates all 4 ECU FreeRTOS tasks and starts the scheduler.
// It is the C entry point called from the Rust host.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

// ── Task entry declarations ───────────────────────────────────────────────

extern void gateway_main(void *pvParameters);
extern void powertrain_main(void *pvParameters);
extern void bms_main(void *pvParameters);
extern void dashboard_main(void *pvParameters);

// ── Stack sizes and priorities ────────────────────────────────────────────

#define GATEWAY_STACK_WORDS    1024
#define POWERTRAIN_STACK_WORDS 1024
#define BMS_STACK_WORDS        1024
#define DASHBOARD_STACK_WORDS  1024

#define GATEWAY_PRIORITY    3
#define POWERTRAIN_PRIORITY 2
#define BMS_PRIORITY        2
#define DASHBOARD_PRIORITY  1

// ── Boot ──────────────────────────────────────────────────────────────────

void microcar_boot(void)
{
    sim_trace_u32("microcar_boot", 1);

    // Create FreeRTOS tasks.  The sim-freertos-port build script patches
    // FreeRTOS's tasks.c to wire task creation into the Rust scheduler.
    xTaskCreate(gateway_main,
                "gateway",
                GATEWAY_STACK_WORDS,
                NULL,
                GATEWAY_PRIORITY,
                NULL);

    xTaskCreate(powertrain_main,
                "powertrain",
                POWERTRAIN_STACK_WORDS,
                NULL,
                POWERTRAIN_PRIORITY,
                NULL);

    xTaskCreate(bms_main,
                "bms",
                BMS_STACK_WORDS,
                NULL,
                BMS_PRIORITY,
                NULL);

    xTaskCreate(dashboard_main,
                "dashboard",
                DASHBOARD_STACK_WORDS,
                NULL,
                DASHBOARD_PRIORITY,
                NULL);

    sim_trace_u32("microcar_tasks_created", 4);

    // For multi-machine Worlds: do NOT call vTaskStartScheduler().
    // The Rust host calls sim_scheduler_tick() which handles fiber
    // creation (first call) and per-tick advancement.
}
