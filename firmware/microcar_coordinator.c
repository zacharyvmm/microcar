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

/// Boot all 4 ECUs on a single machine (for standalone / single-machine tests).
void microcar_boot(void)
{
    sim_trace_u32("microcar_boot", 1);

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
}

/// Boot only the gateway ECU on this machine.
void microcar_boot_gateway(void)
{
    sim_trace_u32("microcar_boot_gateway", 1);
    xTaskCreate(gateway_main, "gateway", GATEWAY_STACK_WORDS, NULL, GATEWAY_PRIORITY, NULL);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot only the powertrain ECU on this machine.
void microcar_boot_powertrain(void)
{
    sim_trace_u32("microcar_boot_powertrain", 1);
    xTaskCreate(powertrain_main, "powertrain", POWERTRAIN_STACK_WORDS, NULL, POWERTRAIN_PRIORITY, NULL);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot only the BMS ECU on this machine.
void microcar_boot_bms(void)
{
    sim_trace_u32("microcar_boot_bms", 1);
    xTaskCreate(bms_main, "bms", BMS_STACK_WORDS, NULL, BMS_PRIORITY, NULL);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot only the dashboard ECU on this machine.
void microcar_boot_dashboard(void)
{
    sim_trace_u32("microcar_boot_dashboard", 1);
    xTaskCreate(dashboard_main, "dashboard", DASHBOARD_STACK_WORDS, NULL, DASHBOARD_PRIORITY, NULL);
    sim_trace_u32("microcar_tasks_created", 1);
}
