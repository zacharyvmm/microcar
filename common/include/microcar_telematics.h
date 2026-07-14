// microcar_telematics.h — telematics protocol definitions (Stage H)
//
// Defines length-prefixed big-endian record format for telematics
// data transmitted over virtual Ethernet device 0.

#ifndef MICROCAR_TELEMATICS_H
#define MICROCAR_TELEMATICS_H

#include <stdint.h>

// ── Telematics record types ────────────────────────────────────────────────

#define TELEM_REC_STATUS    0x01   // periodic status record

// ── Telematics status payload ──────────────────────────────────────────────

typedef struct {
    uint32_t uptime_ms;        // ECU uptime in milliseconds
    uint32_t send_count;       // frames sent since boot
    uint32_t recv_count;       // frames received since boot
} __attribute__((packed)) telem_status_t;

// ── Record wire format ─────────────────────────────────────────────────────
//
// Each record is: [length: uint16_t BE][type: uint8_t][payload: variable]
// where length = sizeof(type) + sizeof(payload).

#define TELEM_RECORD_HEADER_SIZE  3   // 2-byte length + 1-byte type

// Encode a status record into buf (must be at least TELEM_RECORD_HEADER_SIZE + sizeof(telem_status_t) bytes).
// Returns total bytes written.
static inline uint32_t telem_encode_status(uint8_t *buf,
                                           uint32_t uptime_ms,
                                           uint32_t send_count,
                                           uint32_t recv_count)
{
    uint16_t payload_len = sizeof(uint8_t) + sizeof(telem_status_t); // type + payload
    buf[0] = (uint8_t)(payload_len >> 8);
    buf[1] = (uint8_t)(payload_len);
    buf[2] = TELEM_REC_STATUS;
    buf[3] = (uint8_t)(uptime_ms >> 24);
    buf[4] = (uint8_t)(uptime_ms >> 16);
    buf[5] = (uint8_t)(uptime_ms >> 8);
    buf[6] = (uint8_t)(uptime_ms);
    buf[7] = (uint8_t)(send_count >> 24);
    buf[8] = (uint8_t)(send_count >> 16);
    buf[9] = (uint8_t)(send_count >> 8);
    buf[10] = (uint8_t)(send_count);
    buf[11] = (uint8_t)(recv_count >> 24);
    buf[12] = (uint8_t)(recv_count >> 16);
    buf[13] = (uint8_t)(recv_count >> 8);
    buf[14] = (uint8_t)(recv_count);
    return TELEM_RECORD_HEADER_SIZE + sizeof(telem_status_t);
}

#endif // MICROCAR_TELEMATICS_H
