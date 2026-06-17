// microcar_protocol.c — encode/decode helper functions
//
// Provides C functions for encoding and decoding CAN payloads.
// The header defines macros; these functions provide the same
// functionality in callable form for use in firmware.

#include "microcar_protocol.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include <string.h>

// ── Heartbeat ────────────────────────────────────────────────────────────

void mc_encode_heartbeat(uint8_t *buf, uint8_t node_id, uint32_t uptime_ms)
{
    mc_heartbeat_msg_t m = { .node_id = node_id, .uptime_ms = uptime_ms };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_heartbeat(const uint8_t *buf, uint8_t *node_id, uint32_t *uptime_ms)
{
    mc_heartbeat_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *node_id   = m.node_id;
    *uptime_ms = m.uptime_ms;
}

// ── Vehicle Mode ─────────────────────────────────────────────────────────

void mc_encode_vehicle_mode(uint8_t *buf, uint8_t mode, uint8_t fault_code)
{
    mc_vehicle_mode_msg_t m = { .mode = mode, .fault_code = fault_code };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_vehicle_mode(const uint8_t *buf, uint8_t *mode, uint8_t *fault_code)
{
    mc_vehicle_mode_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *mode       = m.mode;
    *fault_code = m.fault_code;
}

// ── Driver Input ─────────────────────────────────────────────────────────

void mc_encode_driver_input(uint8_t *buf, uint8_t throttle, uint8_t brake,
                            uint8_t gear)
{
    mc_driver_input_msg_t m = {
        .throttle_percent = throttle,
        .brake_pressed    = brake,
        .gear             = gear
    };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_driver_input(const uint8_t *buf, uint8_t *throttle,
                            uint8_t *brake, uint8_t *gear)
{
    mc_driver_input_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *throttle = m.throttle_percent;
    *brake    = m.brake_pressed;
    *gear     = m.gear;
}

// ── BMS Status ───────────────────────────────────────────────────────────

void mc_encode_bms_status(uint8_t *buf, uint16_t voltage_mv, int16_t current_ma,
                          int16_t temp_c_x10, uint8_t soc_percent)
{
    mc_bms_status_msg_t m = {
        .pack_voltage_mv  = voltage_mv,
        .pack_current_ma  = current_ma,
        .pack_temp_c_x10  = temp_c_x10,
        .soc_percent      = soc_percent
    };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_bms_status(const uint8_t *buf, uint16_t *voltage_mv,
                          int16_t *current_ma, int16_t *temp_c_x10,
                          uint8_t *soc_percent)
{
    mc_bms_status_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *voltage_mv  = m.pack_voltage_mv;
    *current_ma  = m.pack_current_ma;
    *temp_c_x10  = m.pack_temp_c_x10;
    *soc_percent = m.soc_percent;
}

// ── BMS Limits ───────────────────────────────────────────────────────────

void mc_encode_bms_limits(uint8_t *buf, uint8_t max_torque, uint8_t reason)
{
    mc_bms_limits_msg_t m = {
        .max_torque_percent = max_torque,
        .reason             = reason
    };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_bms_limits(const uint8_t *buf, uint8_t *max_torque,
                          uint8_t *reason)
{
    mc_bms_limits_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *max_torque = m.max_torque_percent;
    *reason     = m.reason;
}

// ── Motor Command ────────────────────────────────────────────────────────

void mc_encode_motor_command(uint8_t *buf, int8_t torque, uint8_t enable)
{
    mc_motor_command_msg_t m = {
        .torque_percent = torque,
        .enable         = enable
    };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_motor_command(const uint8_t *buf, int8_t *torque,
                             uint8_t *enable)
{
    mc_motor_command_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *torque = m.torque_percent;
    *enable = m.enable;
}

// ── Wheel Speed ──────────────────────────────────────────────────────────

void mc_encode_wheel_speed(uint8_t *buf, uint16_t speed_kph_x10)
{
    mc_wheel_speed_msg_t m = { .speed_kph_x10 = speed_kph_x10 };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_wheel_speed(const uint8_t *buf, uint16_t *speed_kph_x10)
{
    mc_wheel_speed_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *speed_kph_x10 = m.speed_kph_x10;
}

// ── Warning ──────────────────────────────────────────────────────────────

void mc_encode_warning(uint8_t *buf, uint8_t source, uint8_t code)
{
    mc_warning_msg_t m = { .source_node = source, .warning_code = code };
    memcpy(buf, &m, sizeof(m));
}

void mc_decode_warning(const uint8_t *buf, uint8_t *source, uint8_t *code)
{
    mc_warning_msg_t m;
    memcpy(&m, buf, sizeof(m));
    *source = m.source_node;
    *code   = m.warning_code;
}

// ── CAN Frame Helpers ────────────────────────────────────────────────────

void mc_frame_init(mc_can_frame_t *frame, uint32_t msg_id, uint8_t sender,
                   uint8_t len)
{
    frame->id     = msg_id;
    frame->sender = sender;
    frame->len    = len;
    memset(frame->data, 0, sizeof(frame->data));
}

int mc_frame_is_valid(const mc_can_frame_t *frame)
{
    return frame->len <= MC_CAN_MAX_DATA;
}

// ── BMS State Determination ──────────────────────────────────────────────

mc_bms_state_t mc_bms_determine_state(int16_t temp_c_x10)
{
    // temp_c_x10 is in 0.1°C units (e.g., 600 = 60.0°C)
    // Thresholds from microcar_safety.h:
    //   MC_BMS_TEMP_OK       = 60 (60.0°C) — below this: BMS_OK
    //   MC_BMS_TEMP_WARN     = 60 — at or above: BMS_WARN_HOT
    //   MC_BMS_TEMP_LIMP     = 75 (75.0°C) — at or above: BMS_LIMP_REQUEST
    //   MC_BMS_TEMP_CRITICAL = 90 (90.0°C) — at or above: BMS_CRITICAL_FAULT

    // Convert thresholds to 0.1°C units
    int16_t t_warn     = (int16_t)(MC_BMS_TEMP_WARN * 10);
    int16_t t_limp     = (int16_t)(MC_BMS_TEMP_LIMP * 10);
    int16_t t_critical = (int16_t)(MC_BMS_TEMP_CRITICAL * 10);

    if (temp_c_x10 >= t_critical) {
        return BMS_CRITICAL_FAULT;
    } else if (temp_c_x10 >= t_limp) {
        return BMS_LIMP_REQUEST;
    } else if (temp_c_x10 >= t_warn) {
        return BMS_WARN_HOT;
    } else {
        return BMS_OK;
    }
}

// ── Torque Safety Limit ──────────────────────────────────────────────────

int8_t mc_safety_clamp_torque(int8_t requested_torque,
                              mc_vehicle_mode_t vehicle_mode,
                              uint8_t brake_pressed,
                              uint8_t bms_max_torque)
{
    // S1: brake overrides throttle → torque = 0
    if (brake_pressed) {
        return 0;
    }

    // S2: FAULT mode → motor disabled
    if (vehicle_mode == VEHICLE_FAULT) {
        return 0;
    }

    // S3: LIMP mode → torque capped at MC_TORQUE_LIMP_MAX_PERCENT (25%)
    int8_t mode_max = 100;
    if (vehicle_mode == VEHICLE_LIMP) {
        mode_max = (int8_t)MC_TORQUE_LIMP_MAX_PERCENT;
    }

    // Apply BMS torque limit if lower
    int8_t effective_max = mode_max;
    if (bms_max_torque < effective_max) {
        effective_max = (int8_t)bms_max_torque;
    }

    // Clamp: must be >= 0 (no negative torque from safety limiter)
    if (requested_torque < 0) {
        requested_torque = 0;
    }
    if (requested_torque > effective_max) {
        requested_torque = effective_max;
    }

    return requested_torque;
}

// ── Gateway Vehicle Mode Transition ──────────────────────────────────────

mc_vehicle_mode_t mc_gateway_determine_mode(mc_vehicle_mode_t current_mode,
                                            uint8_t all_nodes_online,
                                            uint8_t bms_fault_active,
                                            uint8_t bms_limp_requested,
                                            uint8_t powertrain_online)
{
    // FAULT is a terminal state until reboot
    if (current_mode == VEHICLE_FAULT) {
        return VEHICLE_FAULT;
    }

    // Critical BMS fault → FAULT (overrides everything)
    if (bms_fault_active) {
        return VEHICLE_FAULT;
    }

    // CHARGING is a special mode — limp request doesn't apply;
    // only critical fault (handled above) can override it.
    if (current_mode == VEHICLE_CHARGING) {
        return VEHICLE_CHARGING;
    }

    // BMS limp request → LIMP (overrides node online status —
    // the BMS IS communicating if it sent a limp request)
    if (bms_limp_requested) {
        return VEHICLE_LIMP;
    }

    switch (current_mode) {
    case VEHICLE_OFF:
        if (all_nodes_online) {
            return VEHICLE_READY;
        }
        return VEHICLE_OFF;

    case VEHICLE_READY:
        if (!all_nodes_online) {
            return VEHICLE_OFF;
        }
        return VEHICLE_READY;

    case VEHICLE_DRIVE:
        if (!all_nodes_online) {
            return VEHICLE_READY;
        }
        return VEHICLE_DRIVE;

    case VEHICLE_LIMP:
        // LIMP without active limp request → recovering
        if (all_nodes_online) {
            return VEHICLE_READY;
        }
        return VEHICLE_OFF;

    default:
        return VEHICLE_OFF;
    }
}
