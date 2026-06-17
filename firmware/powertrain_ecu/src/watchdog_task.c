// watchdog_task.c — powertrain gateway heartbeat watchdog implementation

#include "watchdog_task.h"
#include <string.h>

void watchdog_init(watchdog_task_t *wd)
{
    memset(wd, 0, sizeof(*wd));
}

void watchdog_gateway_beat(watchdog_task_t *wd, uint32_t now_ms)
{
    wd->last_gateway_beat_ms = now_ms;
    wd->gateway_online       = 1;
    wd->torque_disabled      = 0;
}

uint8_t watchdog_check(watchdog_task_t *wd, uint32_t now_ms)
{
    if (!wd->gateway_online) {
        // Gateway was never online
        wd->torque_disabled = 1;
        return 1;
    }

    uint32_t elapsed = now_ms - wd->last_gateway_beat_ms;
    if (elapsed >= MC_SAFETY_GATEWAY_TIMEOUT_MS) {
        wd->gateway_online  = 0;
        wd->torque_disabled  = 1;
        return 1;
    }

    wd->torque_disabled = 0;
    return 0;
}

uint8_t watchdog_gateway_online(const watchdog_task_t *wd)
{
    return wd->gateway_online;
}

void watchdog_reset(watchdog_task_t *wd)
{
    memset(wd, 0, sizeof(*wd));
}
