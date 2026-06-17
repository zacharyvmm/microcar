// microcar_protocol.h — microcar message protocol definitions
//
// Defines node IDs, message IDs, and payload structs for the
// deterministic CAN-like broadcast bus.

#ifndef MICROCAR_PROTOCOL_H
#define MICROCAR_PROTOCOL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ── Node IDs ─────────────────────────────────────────────────────────────

#define MC_NODE_GATEWAY       1
#define MC_NODE_POWERTRAIN    2
#define MC_NODE_BMS           3
#define MC_NODE_DASHBOARD     4
#define MC_NODE_PLANT         100
#define MC_NODE_TEST_HARNESS  200

// ── Message IDs ──────────────────────────────────────────────────────────

#define MC_MSG_HEARTBEAT          0x001
#define MC_MSG_VEHICLE_MODE       0x010
#define MC_MSG_DRIVER_INPUT       0x020

#define MC_MSG_POWERTRAIN_STATUS  0x100
#define MC_MSG_MOTOR_COMMAND      0x101
#define MC_MSG_WHEEL_SPEED        0x102

#define MC_MSG_BMS_STATUS         0x200
#define MC_MSG_BMS_LIMITS         0x201
#define MC_MSG_BMS_FAULT          0x202

#define MC_MSG_DASHBOARD_STATUS   0x300
#define MC_MSG_WARNING            0x400

// ── Vehicle Modes ────────────────────────────────────────────────────────

typedef enum {
    VEHICLE_OFF       = 0,
    VEHICLE_READY     = 1,
    VEHICLE_DRIVE     = 2,
    VEHICLE_LIMP      = 3,
    VEHICLE_FAULT     = 4,
    VEHICLE_CHARGING  = 5
} mc_vehicle_mode_t;

// ── Powertrain States ────────────────────────────────────────────────────

typedef enum {
    PT_DISABLED = 0,
    PT_READY    = 1,
    PT_DRIVE    = 2,
    PT_LIMP     = 3,
    PT_FAULT    = 4
} mc_powertrain_state_t;

// ── BMS States ───────────────────────────────────────────────────────────

typedef enum {
    BMS_OK              = 0,
    BMS_WARN_HOT        = 1,
    BMS_LIMP_REQUEST    = 2,
    BMS_CRITICAL_FAULT  = 3
} mc_bms_state_t;

// ── Warning Codes ────────────────────────────────────────────────────────

typedef enum {
    MC_WARN_NONE                = 0,
    MC_WARN_BMS_OVERTEMP        = 1,
    MC_WARN_BMS_OFFLINE         = 2,
    MC_WARN_POWERTRAIN_OFFLINE  = 3,
    MC_WARN_GATEWAY_RESTARTED   = 4,
    MC_WARN_DASHBOARD_OFFLINE   = 5,
    MC_WARN_INVALID_THROTTLE    = 6,
    MC_WARN_CRITICAL_BMS_FAULT  = 7,
    MC_WARN_CHARGER_PLUGGED     = 8
} mc_warning_code_t;

// ── BMS Fault Codes ──────────────────────────────────────────────────────

typedef enum {
    MC_BMS_FAULT_NONE          = 0,
    MC_BMS_FAULT_OVERTEMP      = 1,
    MC_BMS_FAULT_OVERVOLTAGE   = 2,
    MC_BMS_FAULT_UNDERVOLTAGE  = 3,
    MC_BMS_FAULT_OVER_CURRENT  = 4,
    MC_BMS_FAULT_COMM_ERROR    = 5
} mc_bms_fault_code_t;

// ── Payload Structs (packed, fixed-size) ─────────────────────────────────

typedef struct {
    uint8_t  node_id;
    uint32_t uptime_ms;
} __attribute__((packed)) mc_heartbeat_msg_t;

typedef struct {
    uint8_t throttle_percent;
    uint8_t brake_pressed;
    uint8_t gear;
} __attribute__((packed)) mc_driver_input_msg_t;

typedef struct {
    uint8_t mode;
    uint8_t fault_code;
} __attribute__((packed)) mc_vehicle_mode_msg_t;

typedef struct {
    uint16_t pack_voltage_mv;
    int16_t  pack_current_ma;
    int16_t  pack_temp_c_x10;
    uint8_t  soc_percent;
} __attribute__((packed)) mc_bms_status_msg_t;

typedef struct {
    uint8_t max_torque_percent;
    uint8_t reason;
} __attribute__((packed)) mc_bms_limits_msg_t;

typedef struct {
    int8_t  torque_percent;
    uint8_t enable;
} __attribute__((packed)) mc_motor_command_msg_t;

typedef struct {
    uint16_t speed_kph_x10;
} __attribute__((packed)) mc_wheel_speed_msg_t;

typedef struct {
    uint8_t source_node;
    uint8_t warning_code;
} __attribute__((packed)) mc_warning_msg_t;

// ── Encode/Decode Macros ─────────────────────────────────────────────────

#define MC_ENCODE_HEARTBEAT(buf, nid, uptime) do { \
    mc_heartbeat_msg_t _m = { .node_id = (nid), .uptime_ms = (uptime) }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_DECODE_HEARTBEAT(buf, nid, uptime) do { \
    mc_heartbeat_msg_t _m; \
    memcpy(&_m, (buf), sizeof(_m)); \
    *(nid) = _m.node_id; \
    *(uptime) = _m.uptime_ms; \
} while (0)

#define MC_ENCODE_VEHICLE_MODE(buf, mode, fault) do { \
    mc_vehicle_mode_msg_t _m = { .mode = (mode), .fault_code = (fault) }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_ENCODE_DRIVER_INPUT(buf, thr, brake, gear_) do { \
    mc_driver_input_msg_t _m = { \
        .throttle_percent = (thr), .brake_pressed = (brake), .gear = (gear_) \
    }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_ENCODE_BMS_STATUS(buf, volt, curr, temp, soc) do { \
    mc_bms_status_msg_t _m = { \
        .pack_voltage_mv = (volt), .pack_current_ma = (curr), \
        .pack_temp_c_x10 = (temp), .soc_percent = (soc) \
    }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_ENCODE_BMS_LIMITS(buf, max_torque, reason) do { \
    mc_bms_limits_msg_t _m = { \
        .max_torque_percent = (max_torque), .reason = (reason) \
    }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_ENCODE_MOTOR_COMMAND(buf, torque, en) do { \
    mc_motor_command_msg_t _m = { .torque_percent = (torque), .enable = (en) }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_ENCODE_WHEEL_SPEED(buf, speed) do { \
    mc_wheel_speed_msg_t _m = { .speed_kph_x10 = (speed) }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

#define MC_ENCODE_WARNING(buf, src, code) do { \
    mc_warning_msg_t _m = { .source_node = (src), .warning_code = (code) }; \
    memcpy((buf), &_m, sizeof(_m)); \
} while (0)

// ── Payload Size Constants ────────────────────────────────────────────────

#define MC_HEARTBEAT_MSG_SIZE       sizeof(mc_heartbeat_msg_t)
#define MC_DRIVER_INPUT_MSG_SIZE    sizeof(mc_driver_input_msg_t)
#define MC_VEHICLE_MODE_MSG_SIZE    sizeof(mc_vehicle_mode_msg_t)
#define MC_BMS_STATUS_MSG_SIZE      sizeof(mc_bms_status_msg_t)
#define MC_BMS_LIMITS_MSG_SIZE      sizeof(mc_bms_limits_msg_t)
#define MC_MOTOR_COMMAND_MSG_SIZE   sizeof(mc_motor_command_msg_t)
#define MC_WHEEL_SPEED_MSG_SIZE     sizeof(mc_wheel_speed_msg_t)
#define MC_WARNING_MSG_SIZE         sizeof(mc_warning_msg_t)

// Maximum payload size (for buffer allocation)
#define MC_MAX_PAYLOAD_SIZE 8

#ifdef __cplusplus
}
#endif

#endif // MICROCAR_PROTOCOL_H
