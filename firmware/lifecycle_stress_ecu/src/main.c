// main.c — lifecycle stress ECU firmware
//
// Tests rapid create/delete cycling (R4a):
//   Supervisor creates 100 workers, each runs a random (deterministic)
//   duration then self-deletes. Verifies fiber count returns to baseline.
//
// FreeRTOS primitives exercised: xTaskCreate (from task body),
// vTaskDelete(NULL).
//
// Note: vTaskSuspend/vTaskResume/vTaskPrioritySet are not currently
// integrated with costar's fiber model, so R4b (suspend/resume) and
// R4c (priority change) are deferred until deeper integration is added.

#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

// ── Simple LCG for deterministic randomness ───────────────────────────────

static uint32_t g_lcg_state = 12345;

static uint32_t lcg_rand(void)
{
    g_lcg_state = g_lcg_state * 1103515245 + 12345;
    return g_lcg_state;
}

// ── R4a: Rapid create/delete worker ───────────────────────────────────────
//
// Each worker runs a random work duration (1-20ms), traces its activity,
// then self-deletes via vTaskDelete(NULL).

void lifecycle_stress_worker(void *pvParameters)
{
    uint32_t id = (uint32_t)(uintptr_t)pvParameters;
    sim_trace_u32("worker_start", id);

    // Random work duration: 1-20ms
    uint32_t work_ms = 1 + (lcg_rand() % 20);
    vTaskDelay(pdMS_TO_TICKS(work_ms));

    sim_trace_u32("worker_done", id);
    vTaskDelete(NULL);
}

// ── R4a: Supervisor ───────────────────────────────────────────────────────
//
// Creates 100 workers in a loop.  Each worker runs a random duration,
// traces, then self-deletes.  The supervisor traces start/done events.

void lifecycle_stress_supervisor(void *pvParameters)
{
    (void)pvParameters;
    sim_register_symbol((uint64_t)xTaskGetCurrentTaskHandle(), "stress_sup");

    sim_trace_u32("stress_create_start", 100);
    for (uint32_t i = 0; i < 100; i++) {
        xTaskCreate(lifecycle_stress_worker,
                    "worker",
                    256,
                    (void *)(uintptr_t)i,
                    1,
                    NULL);
        // Small yield to let workers run
        vTaskDelay(pdMS_TO_TICKS(1));
    }

    // Wait for workers to drain (give them time to finish)
    vTaskDelay(pdMS_TO_TICKS(500));
    sim_trace_u32("stress_create_done", 100);
    vTaskDelete(NULL);
}

// ── Boot entry: Rapid create/delete stress test ──────────────────────────

void microcar_boot_lifecycle_stress(void)
{
    sim_trace_u32("lifecycle_boot", 1);
    TaskHandle_t th;
    xTaskCreate(lifecycle_stress_supervisor, "stress_sup", 1024, NULL, 2, &th);
    sim_bridge_register(sim_create_task("stress_sup", (sim_task_entry_fn)lifecycle_stress_supervisor, NULL, 1024, 2), (void*)th);
    sim_trace_u32("microcar_tasks_created", 1);
}
