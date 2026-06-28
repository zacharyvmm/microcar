// microcar_coordinator.c — Boot entry point for microcar firmware
//
// This file creates all 4 ECU FreeRTOS tasks and starts the scheduler.
// It is the C entry point called from the Rust host.
//
// Pattern: sim_create_task() creates the Rust fiber,
// xTaskCreate() creates the FreeRTOS TCB,
// sim_bridge_register() links them.
// This matches the proven pattern in costar's own demos.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

// ── Task entry declarations ───────────────────────────────────────────────

extern void gateway_main(void *pvParameters);
extern void powertrain_main(void *pvParameters);
extern void bms_main(void *pvParameters);
extern void dashboard_main(void *pvParameters);
extern void net_demo_main(void *pvParameters);
extern void storage_demo_main(void *pvParameters);
extern void bt_demo_main(void *pvParameters);

// ── Stack sizes and priorities ────────────────────────────────────────────

#define GATEWAY_STACK_WORDS    1024
#define POWERTRAIN_STACK_WORDS 1024
#define BMS_STACK_WORDS        1024
#define DASHBOARD_STACK_WORDS  1024
#define DEMO_STACK_WORDS       1024

#define GATEWAY_PRIORITY    3
#define POWERTRAIN_PRIORITY 2
#define BMS_PRIORITY        2
#define DASHBOARD_PRIORITY  1
#define DEMO_PRIORITY       2

// ── Helper: create a task with sim_create_task + xTaskCreate ──────────────

static sim_task_handle_t microcar_create_task(
    const char *name,
    TaskFunction_t entry,
    uint32_t stack_words,
    uint32_t priority)
{
    TaskHandle_t th = NULL;
    xTaskCreate(entry, name, stack_words, NULL, priority, &th);
    sim_task_handle_t h = sim_create_task(name, (sim_task_entry_fn)entry, NULL, stack_words, priority);
    sim_bridge_register(h, (void*)th);
    return h;
}

// ── Boot ──────────────────────────────────────────────────────────────────

/// Boot all 4 ECUs on a single machine (for standalone / single-machine tests).
void microcar_boot(void)
{
    sim_trace_u32("microcar_boot", 1);

    microcar_create_task("gateway",    gateway_main,    GATEWAY_STACK_WORDS,    GATEWAY_PRIORITY);
    microcar_create_task("powertrain", powertrain_main, POWERTRAIN_STACK_WORDS, POWERTRAIN_PRIORITY);
    microcar_create_task("bms",        bms_main,        BMS_STACK_WORDS,        BMS_PRIORITY);
    microcar_create_task("dashboard",  dashboard_main,  DASHBOARD_STACK_WORDS,  DASHBOARD_PRIORITY);

    sim_trace_u32("microcar_tasks_created", 4);
}

/// Boot only the gateway ECU on this machine.
void microcar_boot_gateway(void)
{
    sim_trace_u32("microcar_boot_gateway", 1);
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot only the powertrain ECU on this machine.
void microcar_boot_powertrain(void)
{
    sim_trace_u32("microcar_boot_powertrain", 1);
    microcar_create_task("powertrain", powertrain_main, POWERTRAIN_STACK_WORDS, POWERTRAIN_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot only the BMS ECU on this machine.
void microcar_boot_bms(void)
{
    sim_trace_u32("microcar_boot_bms", 1);
    microcar_create_task("bms", bms_main, BMS_STACK_WORDS, BMS_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot only the dashboard ECU on this machine.
void microcar_boot_dashboard(void)
{
    sim_trace_u32("microcar_boot_dashboard", 1);
    microcar_create_task("dashboard", dashboard_main, DASHBOARD_STACK_WORDS, DASHBOARD_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot the networking demo ECU on this machine.
void microcar_boot_net_demo(void)
{
    sim_trace_u32("microcar_boot_net_demo", 1);
    microcar_create_task("net_demo", net_demo_main, DEMO_STACK_WORDS, DEMO_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot the storage (filesystem) demo ECU on this machine.
void microcar_boot_storage_demo(void)
{
    sim_trace_u32("microcar_boot_storage_demo", 1);
    microcar_create_task("storage_demo", storage_demo_main, DEMO_STACK_WORDS, DEMO_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot the Bluetooth demo ECU on this machine.
void microcar_boot_bt_demo(void)
{
    sim_trace_u32("microcar_boot_bt_demo", 1);
    microcar_create_task("bt_demo", bt_demo_main, DEMO_STACK_WORDS, DEMO_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}
