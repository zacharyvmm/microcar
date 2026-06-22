// Two tasks with vTaskDelay
#include "FreeRTOS.h"
#include "task.h"
#include "sim_abi.h"

static void task_a(void *pvParameters) {
    (void)pvParameters;
    sim_trace_u32("task_a_ran", 1);
    vTaskDelay(pdMS_TO_TICKS(10));
    sim_trace_u32("task_a_delayed", 1);
    vTaskDelete(NULL);
}

static void task_b(void *pvParameters) {
    (void)pvParameters;
    sim_trace_u32("task_b_ran", 2);
    vTaskDelay(pdMS_TO_TICKS(5));
    sim_trace_u32("task_b_delayed", 3);
    vTaskDelete(NULL);
}

void microcar_boot_priority_inversion(void) {
    sim_trace_u32("pi_boot", 1);
    
    TaskHandle_t th;
    xTaskCreate(task_a, "task_a", 256, NULL, 1, &th);
    sim_bridge_register(sim_create_task("task_a", (sim_task_entry_fn)task_a, NULL, 256, 1), (void*)th);
    
    xTaskCreate(task_b, "task_b", 256, NULL, 2, &th);
    sim_bridge_register(sim_create_task("task_b", (sim_task_entry_fn)task_b, NULL, 256, 2), (void*)th);
    
    sim_trace_u32("pi_tasks_created", 2);
}
