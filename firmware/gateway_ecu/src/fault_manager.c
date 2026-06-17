// fault_manager.c — fault aggregation implementation

#include "fault_manager.h"
#include <string.h>

void fault_manager_init(fault_manager_t *fm)
{
    memset(fm, 0, sizeof(*fm));
}

int fault_manager_report(fault_manager_t *fm, uint8_t source_node,
                          uint8_t fault_code, uint8_t severity)
{
    // Check if this fault already exists
    for (uint8_t i = 0; i < fm->fault_count; i++) {
        if (fm->faults[i].source_node == source_node &&
            fm->faults[i].fault_code  == fault_code) {
            // Update existing fault
            if (!fm->faults[i].active) {
                fm->faults[i].active = 1;
                if (severity == 2) fm->critical_count++;
                else if (severity == 1) fm->warning_count++;
            }
            fm->faults[i].severity = severity;
            return 0;
        }
    }

    // New fault
    if (fm->fault_count >= FM_MAX_FAULTS) {
        return -1;
    }

    fm->faults[fm->fault_count].source_node = source_node;
    fm->faults[fm->fault_count].fault_code  = fault_code;
    fm->faults[fm->fault_count].active      = 1;
    fm->faults[fm->fault_count].severity    = severity;
    fm->fault_count++;

    if (severity == 2) fm->critical_count++;
    else if (severity == 1) fm->warning_count++;

    return 0;
}

void fault_manager_clear(fault_manager_t *fm, uint8_t source_node,
                          uint8_t fault_code)
{
    for (uint8_t i = 0; i < fm->fault_count; i++) {
        if (fm->faults[i].source_node == source_node &&
            fm->faults[i].fault_code  == fault_code &&
            fm->faults[i].active) {
            if (fm->faults[i].severity == 2) fm->critical_count--;
            else if (fm->faults[i].severity == 1) fm->warning_count--;
            fm->faults[i].active = 0;
            return;
        }
    }
}

void fault_manager_clear_node(fault_manager_t *fm, uint8_t source_node)
{
    for (uint8_t i = 0; i < fm->fault_count; i++) {
        if (fm->faults[i].source_node == source_node &&
            fm->faults[i].active) {
            if (fm->faults[i].severity == 2) fm->critical_count--;
            else if (fm->faults[i].severity == 1) fm->warning_count--;
            fm->faults[i].active = 0;
        }
    }
}

uint8_t fault_manager_has_critical(const fault_manager_t *fm)
{
    return fm->critical_count > 0 ? 1 : 0;
}

uint8_t fault_manager_active_count(const fault_manager_t *fm)
{
    uint8_t count = 0;
    for (uint8_t i = 0; i < fm->fault_count; i++) {
        if (fm->faults[i].active) {
            count++;
        }
    }
    return count;
}

uint8_t fault_manager_bms_severity(uint8_t bms_fault_code)
{
    switch (bms_fault_code) {
    case MC_BMS_FAULT_OVERTEMP:
    case MC_BMS_FAULT_OVERVOLTAGE:
        return 2; // critical
    case MC_BMS_FAULT_UNDERVOLTAGE:
    case MC_BMS_FAULT_OVER_CURRENT:
        return 1; // warning
    case MC_BMS_FAULT_COMM_ERROR:
        return 2; // critical
    default:
        return 0;
    }
}

uint8_t fault_manager_bms_warning_code(uint8_t bms_fault_code)
{
    switch (bms_fault_code) {
    case MC_BMS_FAULT_OVERTEMP:
        return MC_WARN_BMS_OVERTEMP;
    case MC_BMS_FAULT_OVERVOLTAGE:
    case MC_BMS_FAULT_UNDERVOLTAGE:
        return MC_WARN_BMS_OFFLINE; // generic BMS fault
    case MC_BMS_FAULT_OVER_CURRENT:
    case MC_BMS_FAULT_COMM_ERROR:
        return MC_WARN_CRITICAL_BMS_FAULT;
    default:
        return MC_WARN_NONE;
    }
}
