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
