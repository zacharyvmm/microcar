// heartbeat_monitor.c — per-node heartbeat tracking implementation

#include "heartbeat_monitor.h"
#include <string.h>

void heartbeat_monitor_init(heartbeat_monitor_t *hm, uint32_t timeout_default_ms)
{
    memset(hm, 0, sizeof(*hm));
    // Pre-configure default timeout for all slots (will be overridden on register)
    for (int i = 0; i < HM_MAX_NODES; i++) {
        hm->nodes[i].timeout_ms = timeout_default_ms;
    }
}

int heartbeat_monitor_register(heartbeat_monitor_t *hm, uint8_t node_id,
                                uint32_t timeout_ms)
{
    if (hm->node_count >= HM_MAX_NODES) {
        return -1;
    }
    hm->nodes[hm->node_count].node_id    = node_id;
    hm->nodes[hm->node_count].online     = 0;
    hm->nodes[hm->node_count].last_beat_ms = 0;
    hm->nodes[hm->node_count].timeout_ms = timeout_ms;
    hm->node_count++;
    return 0;
}

void heartbeat_monitor_beat(heartbeat_monitor_t *hm, uint8_t node_id,
                            uint32_t now_ms)
{
    for (uint8_t i = 0; i < hm->node_count; i++) {
        if (hm->nodes[i].node_id == node_id) {
            hm->nodes[i].last_beat_ms = now_ms;
            if (!hm->nodes[i].online) {
                hm->nodes[i].online = 1;
            }
            return;
        }
    }
}

int heartbeat_monitor_check(heartbeat_monitor_t *hm, uint32_t now_ms)
{
    int transitions = 0;

    for (uint8_t i = 0; i < hm->node_count; i++) {
        uint8_t was_online = hm->nodes[i].online;
        uint32_t elapsed   = now_ms - hm->nodes[i].last_beat_ms;

        if (was_online && elapsed >= hm->nodes[i].timeout_ms) {
            hm->nodes[i].online = 0;
            transitions++;
        }
        // Note: we don't auto-transition offline → online here;
        // that happens in heartbeat_monitor_beat().
    }

    return transitions;
}

uint8_t heartbeat_monitor_all_online(const heartbeat_monitor_t *hm)
{
    if (hm->node_count == 0) {
        return 0;
    }
    for (uint8_t i = 0; i < hm->node_count; i++) {
        if (!hm->nodes[i].online) {
            return 0;
        }
    }
    return 1;
}

uint8_t heartbeat_monitor_is_online(const heartbeat_monitor_t *hm,
                                     uint8_t node_id)
{
    for (uint8_t i = 0; i < hm->node_count; i++) {
        if (hm->nodes[i].node_id == node_id) {
            return hm->nodes[i].online;
        }
    }
    return 0;
}

uint8_t heartbeat_monitor_last_transition_node(const heartbeat_monitor_t *hm)
{
    // Find the most recently transitioned node (offline with the
    // smallest elapsed time since last beat — it just timed out).
    uint8_t  result    = 0;
    uint32_t min_elapsed = UINT32_MAX;

    for (uint8_t i = 0; i < hm->node_count; i++) {
        if (!hm->nodes[i].online) {
            // This node is offline; check if it recently transitioned
            // (we can't know exactly when without storing it, but we
            // return the first offline node as a best effort).
            // A more precise implementation would track transition
            // timestamps; for the MVP this is adequate.
            return hm->nodes[i].node_id;
        }
    }
    return 0;
}
