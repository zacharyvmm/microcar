// microcar_ota_slot.h — pure OTA (over-the-air update) slot-metadata model.
//
// A deterministic, side-effect-free A/B-slot update state machine. It carries
// no I/O, no FreeRTOS, and no CAN — only the metadata rules that decide whether
// an update commits or rolls back. Firmware drives it via the functions below;
// the same rules are mirrored (and unit-tested) in Rust at
// state_tests/src/ota_slot.rs, and exercised end-to-end by the microcar
// `ota` dogfood lane.
//
// The state numbering matches the `ota` lane's `ota_state` trace values so a
// scenario can assert the transition sequence directly.

#ifndef MICROCAR_OTA_SLOT_H
#define MICROCAR_OTA_SLOT_H

#include <stdint.h>

// OTA update state machine states.
typedef enum {
    MC_OTA_IDLE           = 0, // no update in progress; running the active slot
    MC_OTA_DOWNLOADING    = 1, // streaming the image into the inactive slot
    MC_OTA_VERIFYING      = 2, // checking image CRC / signature
    MC_OTA_COMMIT_PENDING = 3, // inactive slot armed as the next boot target
    MC_OTA_REBOOTING      = 4, // rebooting to run the new slot
    MC_OTA_HEALTHY        = 5, // new slot passed self-test; update committed
    MC_OTA_ROLLED_BACK    = 6, // update aborted; reverted to the previous good slot
} mc_ota_state_t;

// The two firmware slots (A is the factory / previous-good slot).
#define MC_OTA_SLOT_A 0u
#define MC_OTA_SLOT_B 1u

// Pure OTA slot-metadata model — deterministic state only.
typedef struct {
    uint8_t active_slot;   // slot the bootloader currently runs (last known-good)
    uint8_t target_slot;   // slot being written / armed for the update
    uint8_t state;         // mc_ota_state_t
    uint8_t image_written; // the image finished downloading into target_slot
    uint8_t crc_ok;        // last verify result (1 = image valid)
    uint8_t committed;     // boot flag points at target_slot
    uint8_t boot_healthy;  // post-reboot self-test result (1 = healthy)
    uint8_t rolled_back;   // an update was aborted and the active slot reverted
    uint8_t abort_reason;  // reason code recorded by mc_ota_abort (0 = none)
} mc_ota_slot_state_t;

// Initialise: running slot A, no update in progress.
void mc_ota_init(mc_ota_slot_state_t *s);

// IDLE -> DOWNLOADING. Picks the inactive slot as the target.
void mc_ota_begin_download(mc_ota_slot_state_t *s);

// Report download completion. `complete == 0` models an interrupted write
// (power cut during download): the partial image is discarded and the update
// aborts back to the active slot. Returns 1 if the download completed.
int mc_ota_finish_download(mc_ota_slot_state_t *s, int complete);

// DOWNLOADING -> VERIFYING. Records the CRC/signature result. A failed CRC
// (corrupt image) aborts the update (never arms a bad slot). Returns 1 if the
// image verified OK.
int mc_ota_verify(mc_ota_slot_state_t *s, int crc_ok);

// VERIFYING -> COMMIT_PENDING. Only arms the target slot when the image both
// downloaded fully and verified. Returns 1 if the update was armed.
int mc_ota_commit(mc_ota_slot_state_t *s);

// COMMIT_PENDING -> REBOOTING.
void mc_ota_reboot(mc_ota_slot_state_t *s);

// REBOOTING -> HEALTHY or rollback. `healthy == 0` models a failed post-update
// self-test (bad boot): the model rolls back to the previous good slot.
// Returns 1 if the new slot booted healthy and the update committed permanently.
int mc_ota_health_check(mc_ota_slot_state_t *s, int healthy);

// Abort the in-progress update and revert to the last known-good slot.
// Idempotent and safe from any pre-HEALTHY state; a committed HEALTHY update is
// never undone.
void mc_ota_rollback(mc_ota_slot_state_t *s);

// The slot the bootloader will actually run next: the committed target once a
// healthy update commits, otherwise the (unchanged) active slot.
uint8_t mc_ota_boot_slot(const mc_ota_slot_state_t *s);

// ── Persistent metadata record (Stage F1) ────────────────────────────────
//
// The exact 32-byte little-endian packed record stored in gateway flash pages
// 0 (copy A) and 1 (copy B). `record_crc32` covers bytes 0..27.

#define MC_OTA_META_MAGIC          0x4D434F54u  // 'M','C','O','T'
#define MC_OTA_META_FORMAT_VERSION 1u
#define MC_OTA_META_RECORD_SIZE    32u

typedef struct {
    uint32_t magic;               // 0..3   = MC_OTA_META_MAGIC
    uint8_t  format_version;      // 4      = 1
    uint8_t  active_slot;         // 5
    uint8_t  target_slot;         // 6
    uint8_t  state;               // 7      mc_ota_state_t
    uint8_t  committed;           // 8
    uint8_t  boot_attempt_count;  // 9
    uint8_t  healthy;             // 10
    uint8_t  abort_reason;        // 11
    uint32_t generation;          // 12..15
    uint32_t image_length;        // 16..19
    uint32_t image_crc32;         // 20..23
    uint32_t reserved;            // 24..27 = 0
    uint32_t record_crc32;        // 28..31 over bytes 0..27
} __attribute__((packed)) mc_ota_meta_record_t;

// Reflected IEEE CRC-32 (poly 0xEDB88320, init 0xFFFFFFFF, final XOR
// 0xFFFFFFFF). Used for both the image CRC and the record CRC.
uint32_t mc_ota_crc32(const uint8_t *data, uint32_t len);

// CRC over bytes 0..27 of a record (its `record_crc32` field excluded).
uint32_t mc_ota_record_crc32(const mc_ota_meta_record_t *r);

// 1 if the record has the right magic + format version and its record CRC
// matches the stored value.
int mc_ota_record_valid(const mc_ota_meta_record_t *r);

// Choose the valid record with the highest generation. Returns a pointer to
// the chosen record, or NULL if neither is valid (caller then initialises
// slot A as known-good).
const mc_ota_meta_record_t *mc_ota_select_record(const mc_ota_meta_record_t *a,
                                                 const mc_ota_meta_record_t *b);

// Abort the in-progress update, recording `reason`. Like mc_ota_rollback but
// stores the reason; a committed HEALTHY update is never undone.
void mc_ota_abort(mc_ota_slot_state_t *s, uint8_t reason);

// Reconstruct the RAM model from a persisted metadata record after a reset.
void mc_ota_recover_after_reset(mc_ota_slot_state_t *s,
                                const mc_ota_meta_record_t *r);

#endif // MICROCAR_OTA_SLOT_H
