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
extern void diagnostics_tool_main(void *pvParameters);
extern void gateway_enable_dogfood_diag_script(uint8_t inject_fault);
extern void gateway_enable_dogfood_diag_clear_dtcs(uint8_t buggy);
extern void powertrain_enable_dogfood_service_mode(void);
extern void powertrain_enable_dogfood_service_clamp_bug(void);
extern void gateway_enable_dogfood_charging_script(void);
extern void gateway_enable_dogfood_ota_script(void);
extern void gateway_enable_dogfood_ota_fault_bad_crc(void);
extern void gateway_enable_dogfood_ota_fault_interrupted_write(void);
extern void gateway_enable_dogfood_ota_fault_bad_health(void);
extern void gateway_enable_dogfood_ota_fault_powercut_precommit(void);
extern void gateway_enable_dogfood_ota_bug_bad_crc(void);
extern void powertrain_enable_dogfood_charging(void);
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
#define DIAGNOSTICS_PRIORITY 2

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

/// Boot gateway with the diagnostics dogfood request script enabled.
void microcar_boot_gateway_diag(void)
{
    sim_trace_u32("microcar_boot_gateway_diag", 1);
    gateway_enable_dogfood_diag_script(0);
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with diagnostics script plus a synthetic BMS DTC.
void microcar_boot_gateway_diag_fault(void)
{
    sim_trace_u32("microcar_boot_gateway_diag_fault", 1);
    gateway_enable_dogfood_diag_script(1);
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the clear-DTCs diagnostics script (BMS + powertrain DTCs,
/// correct BMS-scoped clear). The fixed reference for the debug_gym
/// `clear_all_dtcs` seed.
void microcar_boot_gateway_diag_clear(void)
{
    sim_trace_u32("microcar_boot_gateway_diag_clear", 1);
    gateway_enable_dogfood_diag_clear_dtcs(0);
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot the *buggy* clear-DTCs gateway firmware — the debug_gym `clear_all_dtcs`
/// seeded bug. A BMS-scoped CLEAR_DTCS wrongly clears every node's DTCs, so an
/// unrelated powertrain DTC is silently dropped. The fixed reference is
/// microcar_boot_gateway_diag_clear.
void microcar_boot_gateway_diag_clearbug(void)
{
    sim_trace_u32("microcar_boot_gateway_diag_clearbug", 1);
    gateway_enable_dogfood_diag_clear_dtcs(1);
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

/// Boot powertrain forced into SERVICE-mode torque computation.
void microcar_boot_powertrain_diag_service(void)
{
    sim_trace_u32("microcar_boot_powertrain_diag_service", 1);
    powertrain_enable_dogfood_service_mode();
    microcar_create_task("powertrain", powertrain_main, POWERTRAIN_STACK_WORDS, POWERTRAIN_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot the *buggy* powertrain SERVICE firmware — the debug_gym `service_torque`
/// seeded bug. It runs a SERVICE-mode torque computation but skips the safety
/// clamp, so a service session still commands drive torque with the motor
/// enabled. The fixed reference is microcar_boot_powertrain_diag_service.
void microcar_boot_powertrain_diag_service_bug(void)
{
    sim_trace_u32("microcar_boot_powertrain_diag_service_bug", 1);
    powertrain_enable_dogfood_service_clamp_bug();
    microcar_create_task("powertrain", powertrain_main, POWERTRAIN_STACK_WORDS, POWERTRAIN_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the charging dogfood script enabled
/// (plug-in → CHARGING, drive blocked while plugged).
void microcar_boot_gateway_charging(void)
{
    sim_trace_u32("microcar_boot_gateway_charging", 1);
    gateway_enable_dogfood_charging_script();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot powertrain forced into CHARGING-mode torque computation
/// (torque must clamp to 0 while charging).
void microcar_boot_powertrain_charging(void)
{
    sim_trace_u32("microcar_boot_powertrain_charging", 1);
    powertrain_enable_dogfood_charging();
    microcar_create_task("powertrain", powertrain_main, POWERTRAIN_STACK_WORDS, POWERTRAIN_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the OTA (over-the-air update) dogfood script enabled
/// (drives the happy-path OTA state sequence IDLE → … → HEALTHY).
void microcar_boot_gateway_ota(void)
{
    sim_trace_u32("microcar_boot_gateway_ota", 1);
    gateway_enable_dogfood_ota_script();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the OTA bad-CRC fault variant enabled — the corrupt image
/// fails verification, so the slot model rolls back to the known-good slot A
/// (IDLE → DOWNLOADING → VERIFYING[crc bad] → ROLLED_BACK).
void microcar_boot_gateway_ota_badcrc(void)
{
    sim_trace_u32("microcar_boot_gateway_ota_badcrc", 1);
    gateway_enable_dogfood_ota_fault_bad_crc();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the OTA interrupted-write fault variant — a power cut
/// during the image write discards the partial image, so the update aborts to
/// slot A before it ever verifies (IDLE → DOWNLOADING → ROLLED_BACK).
void microcar_boot_gateway_ota_intwrite(void)
{
    sim_trace_u32("microcar_boot_gateway_ota_intwrite", 1);
    gateway_enable_dogfood_ota_fault_interrupted_write();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the OTA bad-health fault variant — the new slot downloads,
/// verifies and commits, but the post-reboot self-test fails, so the model
/// rolls back to slot A (… → REBOOTING → ROLLED_BACK).
void microcar_boot_gateway_ota_badhealth(void)
{
    sim_trace_u32("microcar_boot_gateway_ota_badhealth", 1);
    gateway_enable_dogfood_ota_fault_bad_health();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the OTA power-cut-before-commit fault variant — a valid
/// image is written and verified, but a power cut before the atomic commit
/// discards it and the bootloader stays on slot A (… → VERIFYING → ROLLED_BACK).
void microcar_boot_gateway_ota_powercut(void)
{
    sim_trace_u32("microcar_boot_gateway_ota_powercut", 1);
    gateway_enable_dogfood_ota_fault_powercut_precommit();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
    sim_trace_u32("microcar_tasks_created", 1);
}

/// Boot gateway with the *buggy* OTA CRC-check firmware — the debug_gym
/// `ota_rollback` seeded bug. The image is corrupt, but a broken CRC check
/// accepts it, so the update commits and boots the bad slot (no rollback):
/// IDLE → DOWNLOADING → VERIFYING[crc wrongly OK] → COMMIT_PENDING → REBOOTING
/// → HEALTHY. The fixed reference is microcar_boot_gateway_ota_badcrc, which
/// reports crc_ok=0 and rolls back to slot A.
void microcar_boot_gateway_ota_crcbug(void)
{
    sim_trace_u32("microcar_boot_gateway_ota_crcbug", 1);
    gateway_enable_dogfood_ota_bug_bad_crc();
    microcar_create_task("gateway", gateway_main, GATEWAY_STACK_WORDS, GATEWAY_PRIORITY);
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

/// Boot only the diagnostics tool ECU on this machine.
void microcar_boot_diagnostics_tool(void)
{
    sim_trace_u32("microcar_boot_diagnostics_tool", 1);
    microcar_create_task("diagnostics_tool", diagnostics_tool_main, DEMO_STACK_WORDS, DIAGNOSTICS_PRIORITY);
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
