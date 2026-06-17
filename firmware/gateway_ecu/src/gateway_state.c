// gateway_state.c — gateway ECU vehicle mode state machine implementation

#include "gateway_state.h"
#include <string.h>

void gateway_state_init(gateway_state_t *gs)
{
    memset(gs, 0, sizeof(*gs));
    gs->mode   = VEHICLE_OFF;
    gs->booted = 1;
}

mc_vehicle_mode_t gateway_state_update(gateway_state_t *gs,
                                       uint8_t all_nodes_online,
                                       uint8_t bms_fault_active,
                                       uint8_t bms_limp_requested,
                                       uint8_t fault_count)
{
    gs->all_nodes_online   = all_nodes_online;
    gs->bms_fault_active   = bms_fault_active;
    gs->bms_limp_requested  = bms_limp_requested;
    gs->active_fault_count  = fault_count;

    // Use the shared transition function
    gs->mode = mc_gateway_determine_mode(gs->mode,
                                         all_nodes_online,
                                         bms_fault_active,
                                         bms_limp_requested,
                                         1 /* powertrain_online — we
                                              assume powertrain is always
                                              reachable for state machine
                                              purposes */);
    return gs->mode;
}

void gateway_state_enter_drive(gateway_state_t *gs)
{
    if (gs->mode == VEHICLE_READY) {
        gs->mode = VEHICLE_DRIVE;
    }
}

void gateway_state_reboot(gateway_state_t *gs)
{
    gs->mode               = VEHICLE_OFF;
    gs->all_nodes_online   = 0;
    gs->bms_fault_active   = 0;
    gs->bms_limp_requested  = 0;
    gs->active_fault_count  = 0;
    gs->booted              = 0;
}

const char *gateway_mode_string(mc_vehicle_mode_t mode)
{
    switch (mode) {
    case VEHICLE_OFF:      return MC_MODE_OFF;
    case VEHICLE_READY:    return MC_MODE_READY;
    case VEHICLE_DRIVE:    return MC_MODE_DRIVE;
    case VEHICLE_LIMP:     return MC_MODE_LIMP;
    case VEHICLE_FAULT:    return MC_MODE_FAULT;
    case VEHICLE_CHARGING: return MC_MODE_CHARGING;
    default:               return "UNKNOWN";
    }
}
