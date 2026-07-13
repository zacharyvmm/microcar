// microcar_ota_slot.c — pure OTA slot-metadata model implementation.
//
// See common/include/microcar_ota_slot.h for the contract. This file has no
// dependencies beyond the header: it is deterministic metadata logic that both
// the firmware (gateway OTA dogfood script) and its Rust mirror
// (state_tests/src/ota_slot.rs) implement identically.

#include "microcar_ota_slot.h"

static uint8_t other_slot(uint8_t slot)
{
    return slot == MC_OTA_SLOT_A ? MC_OTA_SLOT_B : MC_OTA_SLOT_A;
}

void mc_ota_init(mc_ota_slot_state_t *s)
{
    if (!s) return;
    s->active_slot   = MC_OTA_SLOT_A;
    s->target_slot   = MC_OTA_SLOT_A;
    s->state         = MC_OTA_IDLE;
    s->image_written = 0;
    s->crc_ok        = 0;
    s->committed     = 0;
    s->boot_healthy  = 0;
    s->rolled_back   = 0;
    s->abort_reason  = 0;
}

void mc_ota_begin_download(mc_ota_slot_state_t *s)
{
    if (!s || s->state != MC_OTA_IDLE) return;
    s->target_slot   = other_slot(s->active_slot);
    s->image_written = 0;
    s->crc_ok        = 0;
    s->committed     = 0;
    s->state         = MC_OTA_DOWNLOADING;
}

int mc_ota_finish_download(mc_ota_slot_state_t *s, int complete)
{
    if (!s || s->state != MC_OTA_DOWNLOADING) return 0;
    if (!complete) {
        // Interrupted write — discard the partial image and abort.
        mc_ota_rollback(s);
        return 0;
    }
    s->image_written = 1;
    return 1;
}

int mc_ota_verify(mc_ota_slot_state_t *s, int crc_ok)
{
    if (!s || s->state != MC_OTA_DOWNLOADING) return 0;
    s->state  = MC_OTA_VERIFYING;
    s->crc_ok = crc_ok ? 1 : 0;
    if (!s->crc_ok) {
        // Corrupt image — never arm a bad slot.
        mc_ota_rollback(s);
        return 0;
    }
    return 1;
}

int mc_ota_commit(mc_ota_slot_state_t *s)
{
    if (!s || s->state != MC_OTA_VERIFYING) return 0;
    if (!s->image_written || !s->crc_ok) {
        mc_ota_rollback(s);
        return 0;
    }
    s->committed = 1;
    s->state     = MC_OTA_COMMIT_PENDING;
    return 1;
}

void mc_ota_reboot(mc_ota_slot_state_t *s)
{
    if (!s || s->state != MC_OTA_COMMIT_PENDING) return;
    s->state = MC_OTA_REBOOTING;
}

int mc_ota_health_check(mc_ota_slot_state_t *s, int healthy)
{
    if (!s || s->state != MC_OTA_REBOOTING) return 0;
    if (healthy && s->committed) {
        s->boot_healthy = 1;
        s->active_slot  = s->target_slot; // update committed permanently
        s->state        = MC_OTA_HEALTHY;
        return 1;
    }
    // Failed self-test — revert to the previous good slot.
    s->boot_healthy = 0;
    mc_ota_rollback(s);
    return 0;
}

void mc_ota_rollback(mc_ota_slot_state_t *s)
{
    if (!s) return;
    if (s->state == MC_OTA_HEALTHY) return; // already committed; nothing to undo
    // active_slot is unchanged: the bootloader keeps running the known-good slot.
    s->target_slot   = s->active_slot;
    s->image_written = 0;
    s->committed     = 0;
    s->boot_healthy  = 0;
    s->rolled_back   = 1;
    s->state         = MC_OTA_ROLLED_BACK;
}

uint8_t mc_ota_boot_slot(const mc_ota_slot_state_t *s)
{
    if (!s) return MC_OTA_SLOT_A;
    if (s->state == MC_OTA_HEALTHY && s->committed) return s->target_slot;
    return s->active_slot;
}

// ── Stage F1: persistent metadata record ─────────────────────────────────

uint32_t mc_ota_crc32(const uint8_t *data, uint32_t len)
{
    uint32_t crc = 0xFFFFFFFFu;
    for (uint32_t i = 0; i < len; i++) {
        crc ^= data[i];
        for (int b = 0; b < 8; b++) {
            uint32_t mask = (uint32_t)(-(int32_t)(crc & 1u));
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return crc ^ 0xFFFFFFFFu;
}

uint32_t mc_ota_record_crc32(const mc_ota_meta_record_t *r)
{
    if (!r) return 0;
    return mc_ota_crc32((const uint8_t *)r, 28u);
}

int mc_ota_record_valid(const mc_ota_meta_record_t *r)
{
    if (!r) return 0;
    if (r->magic != MC_OTA_META_MAGIC) return 0;
    if (r->format_version != MC_OTA_META_FORMAT_VERSION) return 0;
    return r->record_crc32 == mc_ota_record_crc32(r);
}

const mc_ota_meta_record_t *mc_ota_select_record(const mc_ota_meta_record_t *a,
                                                 const mc_ota_meta_record_t *b)
{
    int va = mc_ota_record_valid(a);
    int vb = mc_ota_record_valid(b);
    if (va && vb) {
        return (a->generation >= b->generation) ? a : b;
    }
    if (va) return a;
    if (vb) return b;
    return 0;
}

void mc_ota_abort(mc_ota_slot_state_t *s, uint8_t reason)
{
    if (!s) return;
    if (s->state == MC_OTA_HEALTHY) return; // committed update never undone
    mc_ota_rollback(s);
    s->abort_reason = reason;
}

void mc_ota_recover_after_reset(mc_ota_slot_state_t *s,
                                const mc_ota_meta_record_t *r)
{
    if (!s || !r) return;
    s->active_slot   = r->active_slot;
    s->target_slot   = r->target_slot;
    s->state         = r->state;
    s->committed     = r->committed ? 1 : 0;
    s->boot_healthy  = r->healthy ? 1 : 0;
    s->abort_reason  = r->abort_reason;
    // Derived RAM-only flags: a committed target has a written, verified image.
    s->image_written = (r->state >= MC_OTA_COMMIT_PENDING) ? 1 : 0;
    s->crc_ok        = (r->state >= MC_OTA_COMMIT_PENDING) ? 1 : 0;
    s->rolled_back   = (r->state == MC_OTA_ROLLED_BACK) ? 1 : 0;
}
