// fault_manager.h — fault aggregation and warning publication
//
// Collects faults from all ECUs, tracks fault counts, and determines
// when to escalate to gateway state transitions.

#ifndef FAULT_MANAGER_H
#define FAULT_MANAGER_H

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Maximum number of distinct faults tracked simultaneously.
#define FM_MAX_FAULTS 16

/// A single tracked fault.
typedef struct {
    uint8_t source_node;    // MC_NODE_* that reported the fault
    uint8_t fault_code;     // fault-specific code (e.g., MC_BMS_FAULT_OVERTEMP)
    uint8_t active;         // 1 if fault is still active
    uint8_t severity;       // 0=info, 1=warning, 2=critical
} fm_fault_t;

/// Fault manager state.
typedef struct {
    fm_fault_t faults[FM_MAX_FAULTS];
    uint8_t    fault_count;
    uint8_t    critical_count;
    uint8_t    warning_count;
} fault_manager_t;

/// Initialise the fault manager.
void fault_manager_init(fault_manager_t *fm);

/// Report or update a fault from a source node.
/// If the fault is already tracked, updates its active/severity.
/// Returns 0 on success, -1 if fault table is full.
int  fault_manager_report(fault_manager_t *fm, uint8_t source_node,
                          uint8_t fault_code, uint8_t severity);

/// Clear (resolve) a specific fault.
void fault_manager_clear(fault_manager_t *fm, uint8_t source_node,
                         uint8_t fault_code);

/// Clear all faults from a source node (e.g., after a reboot).
void fault_manager_clear_node(fault_manager_t *fm, uint8_t source_node);

/// Returns 1 if any critical-severity fault is active.
uint8_t fault_manager_has_critical(const fault_manager_t *fm);

/// Returns the total number of active faults.
uint8_t fault_manager_active_count(const fault_manager_t *fm);

/// Determine severity level from a BMS fault code.
uint8_t fault_manager_bms_severity(uint8_t bms_fault_code);

/// Determine the appropriate warning_code from a BMS fault.
uint8_t fault_manager_bms_warning_code(uint8_t bms_fault_code);

#ifdef __cplusplus
}
#endif

#endif // FAULT_MANAGER_H
