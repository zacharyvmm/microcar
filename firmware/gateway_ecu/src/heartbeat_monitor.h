// heartbeat_monitor.h — per-node heartbeat tracking
//
// Each ECU sends a heartbeat every MC_SAFETY_HEARTBEAT_INTERVAL_MS (100ms).
// The gateway monitors these and declares nodes offline after timeout.

#ifndef HEARTBEAT_MONITOR_H
#define HEARTBEAT_MONITOR_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Maximum number of nodes the gateway monitors.
#define HM_MAX_NODES 8

/// Per-node heartbeat tracking entry.
typedef struct {
    uint8_t  node_id;        // MC_NODE_* constant
    uint8_t  online;         // 1 if last heartbeat was within timeout
    uint32_t last_beat_ms;   // virtual time of last received heartbeat
    uint32_t timeout_ms;     // timeout threshold for this node
} hm_node_t;

/// Heartbeat monitor state.
typedef struct {
    hm_node_t nodes[HM_MAX_NODES];
    uint8_t   node_count;
} heartbeat_monitor_t;

/// Initialise the heartbeat monitor.
/// timeout_default_ms: default timeout for all nodes (e.g., MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS)
void heartbeat_monitor_init(heartbeat_monitor_t *hm, uint32_t timeout_default_ms);

/// Register a node to monitor. Returns 0 on success, -1 if full.
int  heartbeat_monitor_register(heartbeat_monitor_t *hm, uint8_t node_id,
                                uint32_t timeout_ms);

/// Record a heartbeat from a node at a given virtual time.
void heartbeat_monitor_beat(heartbeat_monitor_t *hm, uint8_t node_id,
                            uint32_t now_ms);

/// Check all nodes for timeouts and update online/offline status.
/// Returns the number of nodes that transitioned (online→offline or offline→online).
int  heartbeat_monitor_check(heartbeat_monitor_t *hm, uint32_t now_ms);

/// Returns 1 if all registered nodes are online, 0 otherwise.
uint8_t heartbeat_monitor_all_online(const heartbeat_monitor_t *hm);

/// Returns 1 if a specific node is online, 0 otherwise.
uint8_t heartbeat_monitor_is_online(const heartbeat_monitor_t *hm,
                                    uint8_t node_id);

/// Get the node_id of a node that just went offline (for trace events).
/// Returns 0 if no recent transition.
uint8_t heartbeat_monitor_last_transition_node(const heartbeat_monitor_t *hm);

#ifdef __cplusplus
}
#endif

#endif // HEARTBEAT_MONITOR_H
