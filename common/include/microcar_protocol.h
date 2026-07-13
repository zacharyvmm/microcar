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
#define MC_NODE_DIAGNOSTICS   5
#define MC_NODE_PLANT         100
#define MC_NODE_TEST_HARNESS  200

// External protocol actors (Stage C). EVSE charger, OTA tool, telematics unit.
#define MC_NODE_EVSE          6
#define MC_NODE_OTA_TOOL      7
#define MC_NODE_TELEMATICS    8

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

// Plant-published sensor data (SOC, voltage, temperature, current).
#define MC_MSG_PLANT_SENSORS      0x500

// Lightweight diagnostics request/response, used by the dogfood diagnostics
// lane. Payloads keep byte 0 as sender id so C firmware can recover the source
// from sim_can_recv() payloads.
#define MC_MSG_DIAG_REQUEST       0x600
#define MC_MSG_DIAG_RESPONSE      0x601

// ── Charging / EVSE (Stage C) ─────────────────────────────────────────────
// BMS-published charge current/thermal limit (source = MC_NODE_BMS).
#define MC_MSG_BMS_CHARGE_LIMIT   0x203
// EVSE charger events (source = MC_NODE_EVSE).
#define MC_MSG_EVSE_EVENT         0x610
// Gateway charge command to plant/BMS (source = MC_NODE_GATEWAY).
#define MC_MSG_CHARGE_COMMAND     0x611

// ── OTA (Stage C, tool protocol 0x630..0x633) ─────────────────────────────
#define MC_MSG_OTA_REQUEST        0x630   // source = MC_NODE_OTA_TOOL
#define MC_MSG_OTA_CHUNK          0x631   // source = MC_NODE_OTA_TOOL
#define MC_MSG_OTA_FINISH         0x632   // source = MC_NODE_OTA_TOOL
#define MC_MSG_OTA_STATUS         0x633   // source = MC_NODE_GATEWAY

// ── Vehicle Modes ────────────────────────────────────────────────────────

typedef enum {
    VEHICLE_OFF       = 0,
    VEHICLE_READY     = 1,
    VEHICLE_DRIVE     = 2,
    VEHICLE_LIMP      = 3,
    VEHICLE_FAULT     = 4,
    VEHICLE_CHARGING  = 5,
    VEHICLE_SERVICE   = 6,
    VEHICLE_OTA_UPDATE = 7,
    VEHICLE_TRANSPORT_MODE = 8
} mc_vehicle_mode_t;

// ── Diagnostics services ───────────────────────────────────────────────

typedef enum {
    MC_DIAG_START_SESSION = 1,
    MC_DIAG_READ_MODE     = 2,
    MC_DIAG_READ_DTCS     = 3,
    MC_DIAG_CLEAR_DTCS    = 4,
    MC_DIAG_LIVE_BMS      = 5,
    MC_DIAG_ACTUATOR_TEST = 6,
    MC_DIAG_END_SESSION   = 7
} mc_diag_service_t;

typedef enum {
    MC_DIAG_OK          = 0,
    MC_DIAG_REJECTED    = 1,
    MC_DIAG_UNSUPPORTED = 2,
    MC_DIAG_STALE       = 3
} mc_diag_status_t;

typedef struct {
    uint8_t source_node;
    uint8_t service;
    uint8_t request_id;
    uint8_t param;
} __attribute__((packed)) mc_diag_request_msg_t;

typedef struct {
    uint8_t source_node;
    uint8_t service;
    uint8_t request_id;
    uint8_t status;
    uint8_t value0;
    uint8_t value1;
} __attribute__((packed)) mc_diag_response_msg_t;

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

// ── EVSE / Charging enums (Stage C) ──────────────────────────────────────

typedef enum {
    MC_EVSE_UNPLUG       = 0,
    MC_EVSE_PLUG         = 1,
    MC_EVSE_HANDSHAKE_OK = 2,
    MC_EVSE_STOP         = 3
} mc_evse_event_t;

typedef enum {
    MC_CHARGE_DISCONNECTED  = 0,
    MC_CHARGE_PLUG_DETECTED = 1,
    MC_CHARGE_HANDSHAKE     = 2,
    MC_CHARGE_ACTIVE        = 3,
    MC_CHARGE_LIMITED       = 4,
    MC_CHARGE_COMPLETE      = 5,
    MC_CHARGE_FAULT         = 6
} mc_charge_state_t;

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

// ── Stage C payload structs (packed, exact wire layouts) ─────────────────

// 0x203 BMS_CHARGE_LIMIT (7 bytes): source(=3), max_current_a_x2 (0.5 A units),
// soc_percent, temp_c_x10 (LE i16), fault, seq.
typedef struct {
    uint8_t source_node;
    uint8_t max_current_a_x2;
    uint8_t soc_percent;
    int16_t temp_c_x10;
    uint8_t fault;
    uint8_t seq;
} __attribute__((packed)) mc_bms_charge_limit_msg_t;

// 0x610 EVSE_EVENT (6 bytes): source(=6), event, request_id,
// offered_current_a_x2, target_soc, reserved.
typedef struct {
    uint8_t source_node;
    uint8_t event;
    uint8_t request_id;
    uint8_t offered_current_a_x2;
    uint8_t target_soc;
    uint8_t reserved;
} __attribute__((packed)) mc_evse_event_msg_t;

// 0x611 CHARGE_COMMAND (6 bytes): source(=1), state, request_id, current_a_x2,
// target_soc, reason.
typedef struct {
    uint8_t source_node;
    uint8_t state;
    uint8_t request_id;
    uint8_t current_a_x2;
    uint8_t target_soc;
    uint8_t reason;
} __attribute__((packed)) mc_charge_command_msg_t;

// 0x630 OTA_REQUEST (4 bytes): source(=7), request_id, image_id, total_chunks.
typedef struct {
    uint8_t source_node;
    uint8_t request_id;
    uint8_t image_id;
    uint8_t total_chunks;
} __attribute__((packed)) mc_ota_request_msg_t;

// 0x631 OTA_CHUNK (8 bytes): source(=7), request_id, chunk_index, data[5].
typedef struct {
    uint8_t source_node;
    uint8_t request_id;
    uint8_t chunk_index;
    uint8_t data[5];
} __attribute__((packed)) mc_ota_chunk_msg_t;

// 0x632 OTA_FINISH (7 bytes): source(=7), request_id, total_chunks,
// crc32 (LE u32).
typedef struct {
    uint8_t  source_node;
    uint8_t  request_id;
    uint8_t  total_chunks;
    uint32_t crc32;
} __attribute__((packed)) mc_ota_finish_msg_t;

// 0x633 OTA_STATUS (8 bytes): source(=1), request_id, state, status,
// active_slot, target_slot, reason, seq.
typedef struct {
    uint8_t source_node;
    uint8_t request_id;
    uint8_t state;
    uint8_t status;
    uint8_t active_slot;
    uint8_t target_slot;
    uint8_t reason;
    uint8_t seq;
} __attribute__((packed)) mc_ota_status_msg_t;

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
#define MC_DIAG_REQUEST_MSG_SIZE    sizeof(mc_diag_request_msg_t)
#define MC_DIAG_RESPONSE_MSG_SIZE   sizeof(mc_diag_response_msg_t)

// Maximum payload size (for buffer allocation)
#define MC_MAX_PAYLOAD_SIZE 8

// ── Functions ──────────────────────────────────────────────────────

/// Gateway vehicle mode state transition function.
mc_vehicle_mode_t mc_gateway_determine_mode(mc_vehicle_mode_t current_mode,
                                            uint8_t all_nodes_online,
                                            uint8_t bms_fault_active,
                                            uint8_t bms_limp_requested,
                                            uint8_t powertrain_online);

/// BMS fault severity lookup.
uint8_t fault_manager_bms_severity(uint8_t fault_code);

/// BMS state determination from temperature.
mc_bms_state_t mc_bms_determine_state(int16_t temp_c_x10);

#ifdef __cplusplus
}
#endif

#endif // MICROCAR_PROTOCOL_H
