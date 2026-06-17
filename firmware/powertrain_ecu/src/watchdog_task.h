// watchdog_task.h — powertrain gateway heartbeat watchdog
//
// S4: If gateway heartbeat is lost, powertrain must disable torque
//     within MC_SAFETY_GATEWAY_TIMEOUT_MS (250ms).

#ifndef WATCHDOG_TASK_H
#define WATCHDOG_TASK_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Watchdog state tracking gateway heartbeat.
typedef struct {
    uint32_t last_gateway_beat_ms;  // virtual time of last gateway heartbeat
    uint8_t  gateway_online;         // 1 if gateway is communicating
    uint8_t  torque_disabled;        // 1 if torque was disabled by watchdog
} watchdog_task_t;

/// Initialise the watchdog.
void watchdog_init(watchdog_task_t *wd);

/// Record a gateway heartbeat at the given virtual time.
void watchdog_gateway_beat(watchdog_task_t *wd, uint32_t now_ms);

/// Check for gateway timeout. If the gateway has been silent for
/// more than MC_SAFETY_GATEWAY_TIMEOUT_MS, disable torque.
/// Returns 1 if torque should be disabled, 0 otherwise.
uint8_t watchdog_check(watchdog_task_t *wd, uint32_t now_ms);

/// Returns 1 if the gateway is currently considered online.
uint8_t watchdog_gateway_online(const watchdog_task_t *wd);

/// Reset the watchdog (e.g., after a reboot).
void watchdog_reset(watchdog_task_t *wd);

#ifdef __cplusplus
}
#endif

#endif // WATCHDOG_TASK_H
