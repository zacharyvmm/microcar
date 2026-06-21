// microcar_can.h — CAN-like broadcast bus frame definition
//
// Lightweight CAN frame for the deterministic broadcast bus.
// Each ECU sends/receives frames through this structure.

#ifndef MICROCAR_CAN_H
#define MICROCAR_CAN_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Maximum data length in a CAN frame.
#define MC_CAN_MAX_DATA 8

/// A single CAN-like frame.
typedef struct {
    uint32_t id;          // Message ID (lower = higher priority)
    uint8_t  sender;      // Source node ID
    uint8_t  data[MC_CAN_MAX_DATA];  // Payload
    uint8_t  len;         // Actual payload length (0..8)
} __attribute__((packed)) mc_can_frame_t;

#ifdef __cplusplus
}
#endif

/// Initialize a CAN frame with message ID, sender, and payload length.
/// Does NOT fill the data bytes — caller must do that.
void mc_frame_init(mc_can_frame_t *frame,
                   uint32_t msg_id, uint8_t sender, uint8_t len);

#endif // MICROCAR_CAN_H
