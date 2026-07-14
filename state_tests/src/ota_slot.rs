//! ota_slot.rs — pure OTA (over-the-air update) A/B slot-metadata model.
//!
//! Mirrors the C model in `common/src/microcar_ota_slot.c` exactly: a
//! deterministic, side-effect-free state machine that decides whether a
//! firmware update commits or rolls back. No FreeRTOS, no I/O — just the
//! metadata rules, so the commit/rollback contract can be unit-tested here
//! without the simulator. The firmware (gateway OTA dogfood script) drives the
//! C implementation; the microcar `ota` dogfood lane exercises the same rules
//! end-to-end. Keep this file in sync with the C model if either changes.

/// OTA update state machine states. Values match the `ota` lane's `ota_state`
/// trace numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OtaState {
    Idle = 0,
    Downloading = 1,
    Verifying = 2,
    CommitPending = 3,
    Rebooting = 4,
    Healthy = 5,
    RolledBack = 6,
}

pub const SLOT_A: u8 = 0;
pub const SLOT_B: u8 = 1;

fn other_slot(slot: u8) -> u8 {
    if slot == SLOT_A {
        SLOT_B
    } else {
        SLOT_A
    }
}

/// Pure OTA slot-metadata model — deterministic state only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaSlotModel {
    /// Slot the bootloader currently runs (last known-good).
    pub active_slot: u8,
    /// Slot being written / armed for the update.
    pub target_slot: u8,
    pub state: OtaState,
    /// The image finished downloading into `target_slot`.
    pub image_written: bool,
    /// Last verify result (`true` = image valid).
    pub crc_ok: bool,
    /// Boot flag points at `target_slot`.
    pub committed: bool,
    /// Post-reboot self-test result (`true` = healthy).
    pub boot_healthy: bool,
    /// An update was aborted and the active slot reverted.
    pub rolled_back: bool,
    /// Reason code recorded by [`OtaSlotModel::abort`] (0 = none).
    pub abort_reason: u8,
}

impl Default for OtaSlotModel {
    fn default() -> Self {
        Self::new()
    }
}

impl OtaSlotModel {
    /// Initialise: running slot A, no update in progress.
    pub fn new() -> Self {
        Self {
            active_slot: SLOT_A,
            target_slot: SLOT_A,
            state: OtaState::Idle,
            image_written: false,
            crc_ok: false,
            committed: false,
            boot_healthy: false,
            rolled_back: false,
            abort_reason: 0,
        }
    }

    /// IDLE -> DOWNLOADING. Picks the inactive slot as the target.
    pub fn begin_download(&mut self) {
        if self.state != OtaState::Idle {
            return;
        }
        self.target_slot = other_slot(self.active_slot);
        self.image_written = false;
        self.crc_ok = false;
        self.committed = false;
        self.state = OtaState::Downloading;
    }

    /// Report download completion. `complete == false` models an interrupted
    /// write (power cut during download): the partial image is discarded and
    /// the update aborts back to the active slot. Returns `true` if the
    /// download completed.
    pub fn finish_download(&mut self, complete: bool) -> bool {
        if self.state != OtaState::Downloading {
            return false;
        }
        if !complete {
            self.rollback();
            return false;
        }
        self.image_written = true;
        true
    }

    /// DOWNLOADING -> VERIFYING. Records the CRC/signature result. A failed CRC
    /// (corrupt image) aborts the update. Returns `true` if the image verified.
    pub fn verify(&mut self, crc_ok: bool) -> bool {
        if self.state != OtaState::Downloading {
            return false;
        }
        self.state = OtaState::Verifying;
        self.crc_ok = crc_ok;
        if !crc_ok {
            self.rollback();
            return false;
        }
        true
    }

    /// VERIFYING -> COMMIT_PENDING. Only arms the target slot when the image
    /// both downloaded fully and verified. Returns `true` if armed.
    pub fn commit(&mut self) -> bool {
        if self.state != OtaState::Verifying {
            return false;
        }
        if !self.image_written || !self.crc_ok {
            self.rollback();
            return false;
        }
        self.committed = true;
        self.state = OtaState::CommitPending;
        true
    }

    /// COMMIT_PENDING -> REBOOTING.
    pub fn reboot(&mut self) {
        if self.state != OtaState::CommitPending {
            return;
        }
        self.state = OtaState::Rebooting;
    }

    /// REBOOTING -> HEALTHY or rollback. `healthy == false` models a failed
    /// post-update self-test (bad boot): the model rolls back to the previous
    /// good slot. Returns `true` if the new slot booted healthy and committed.
    pub fn health_check(&mut self, healthy: bool) -> bool {
        if self.state != OtaState::Rebooting {
            return false;
        }
        if healthy && self.committed {
            self.boot_healthy = true;
            self.active_slot = self.target_slot; // update committed permanently
            self.state = OtaState::Healthy;
            return true;
        }
        self.boot_healthy = false;
        self.rollback();
        false
    }

    /// Abort the in-progress update and revert to the last known-good slot.
    /// Idempotent; a committed HEALTHY update is never undone.
    pub fn rollback(&mut self) {
        if self.state == OtaState::Healthy {
            return;
        }
        // active_slot is unchanged: the bootloader keeps the known-good slot.
        self.target_slot = self.active_slot;
        self.image_written = false;
        self.committed = false;
        self.boot_healthy = false;
        self.rolled_back = true;
        self.state = OtaState::RolledBack;
    }

    /// The slot the bootloader will actually run next: the committed target once
    /// a healthy update commits, otherwise the (unchanged) active slot.
    pub fn boot_slot(&self) -> u8 {
        if self.state == OtaState::Healthy && self.committed {
            self.target_slot
        } else {
            self.active_slot
        }
    }

    /// Abort the in-progress update, recording `reason`. Like [`rollback`] but
    /// stores the reason; a committed HEALTHY update is never undone.
    ///
    /// [`rollback`]: OtaSlotModel::rollback
    pub fn abort(&mut self, reason: u8) {
        if self.state == OtaState::Healthy {
            return;
        }
        self.rollback();
        self.abort_reason = reason;
    }

    /// Reconstruct the RAM model from a persisted metadata record after a reset.
    pub fn recover_after_reset(&mut self, r: &MetaRecord) {
        self.active_slot = r.active_slot;
        self.target_slot = r.target_slot;
        self.state = OtaState::from_u8(r.state);
        self.committed = r.committed != 0;
        self.boot_healthy = r.healthy != 0;
        self.abort_reason = r.abort_reason;
        let committed_or_later = r.state >= OtaState::CommitPending as u8;
        self.image_written = committed_or_later;
        self.crc_ok = committed_or_later;
        self.rolled_back = r.state == OtaState::RolledBack as u8;
    }
}

impl OtaState {
    /// Map a raw state byte (as stored in a metadata record) to an [`OtaState`].
    pub fn from_u8(v: u8) -> OtaState {
        match v {
            0 => OtaState::Idle,
            1 => OtaState::Downloading,
            2 => OtaState::Verifying,
            3 => OtaState::CommitPending,
            4 => OtaState::Rebooting,
            5 => OtaState::Healthy,
            _ => OtaState::RolledBack,
        }
    }
}

// ── Stage F1: persistent metadata record ─────────────────────────────────

pub const OTA_META_MAGIC: u32 = 0x4D43_4F54; // 'M','C','O','T'
pub const OTA_META_FORMAT_VERSION: u8 = 1;
pub const OTA_META_RECORD_SIZE: usize = 32;

/// Reflected IEEE CRC-32 (poly 0xEDB88320, init 0xFFFFFFFF, final XOR
/// 0xFFFFFFFF). Mirror of `mc_ota_crc32`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc ^ 0xFFFF_FFFF
}

/// The exact 32-byte little-endian metadata record (mirror of
/// `mc_ota_meta_record_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetaRecord {
    pub magic: u32,
    pub format_version: u8,
    pub active_slot: u8,
    pub target_slot: u8,
    pub state: u8,
    pub committed: u8,
    pub boot_attempt_count: u8,
    pub healthy: u8,
    pub abort_reason: u8,
    pub generation: u32,
    pub image_length: u32,
    pub image_crc32: u32,
    pub reserved: u32,
    pub record_crc32: u32,
}

impl MetaRecord {
    /// Serialize into the 32-byte little-endian on-flash layout.
    pub fn to_bytes(&self) -> [u8; OTA_META_RECORD_SIZE] {
        let mut b = [0u8; OTA_META_RECORD_SIZE];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4] = self.format_version;
        b[5] = self.active_slot;
        b[6] = self.target_slot;
        b[7] = self.state;
        b[8] = self.committed;
        b[9] = self.boot_attempt_count;
        b[10] = self.healthy;
        b[11] = self.abort_reason;
        b[12..16].copy_from_slice(&self.generation.to_le_bytes());
        b[16..20].copy_from_slice(&self.image_length.to_le_bytes());
        b[20..24].copy_from_slice(&self.image_crc32.to_le_bytes());
        b[24..28].copy_from_slice(&self.reserved.to_le_bytes());
        b[28..32].copy_from_slice(&self.record_crc32.to_le_bytes());
        b
    }

    /// CRC over bytes 0..27 (excludes the `record_crc32` field).
    pub fn compute_record_crc32(&self) -> u32 {
        crc32(&self.to_bytes()[0..28])
    }

    /// Stamp magic/version and the record CRC, producing a valid record.
    pub fn finalize(&mut self) {
        self.magic = OTA_META_MAGIC;
        self.format_version = OTA_META_FORMAT_VERSION;
        self.record_crc32 = self.compute_record_crc32();
    }

    /// Whether the record has the right magic + format and a matching CRC.
    pub fn is_valid(&self) -> bool {
        self.magic == OTA_META_MAGIC
            && self.format_version == OTA_META_FORMAT_VERSION
            && self.record_crc32 == self.compute_record_crc32()
    }
}

/// Choose the valid record with the highest generation (mirror of
/// `mc_ota_select_record`). `None` when neither is valid.
pub fn select_record(a: &MetaRecord, b: &MetaRecord) -> Option<MetaRecord> {
    match (a.is_valid(), b.is_valid()) {
        (true, true) => Some(if a.generation >= b.generation { *a } else { *b }),
        (true, false) => Some(*a),
        (false, true) => Some(*b),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Happy path: download -> verify -> commit -> reboot -> healthy.
    /// The update commits permanently and the active slot flips A -> B.
    #[test]
    fn happy_path_commits_to_slot_b() {
        let mut m = OtaSlotModel::new();
        assert_eq!(m.active_slot, SLOT_A);

        m.begin_download();
        assert_eq!(m.state, OtaState::Downloading);
        assert_eq!(m.target_slot, SLOT_B);

        assert!(m.finish_download(true));
        assert!(m.verify(true));
        assert_eq!(m.state, OtaState::Verifying);

        assert!(m.commit());
        assert_eq!(m.state, OtaState::CommitPending);

        m.reboot();
        assert_eq!(m.state, OtaState::Rebooting);

        assert!(m.health_check(true));
        assert_eq!(m.state, OtaState::Healthy);
        assert!(m.boot_healthy);
        assert!(!m.rolled_back);
        // The update committed permanently: bootloader now runs slot B.
        assert_eq!(m.active_slot, SLOT_B);
        assert_eq!(m.boot_slot(), SLOT_B);
    }

    /// Corrupt image: a failed CRC at verify must abort the update and never
    /// arm the bad slot — active slot stays A.
    #[test]
    fn corrupt_image_rolls_back_and_keeps_slot_a() {
        let mut m = OtaSlotModel::new();
        m.begin_download();
        assert!(m.finish_download(true));

        assert!(!m.verify(false));
        assert!(!m.crc_ok);
        assert_eq!(m.state, OtaState::RolledBack);
        assert!(m.rolled_back);

        // Commit must be a no-op after rollback.
        assert!(!m.commit());
        assert_eq!(m.state, OtaState::RolledBack);

        // The bootloader keeps running the known-good slot A.
        assert_eq!(m.active_slot, SLOT_A);
        assert_eq!(m.boot_slot(), SLOT_A);
    }

    /// Failed post-update self-test (bad boot): the new slot boots unhealthy, so
    /// the model rolls back to the previous good slot A.
    #[test]
    fn failed_health_check_rolls_back_to_slot_a() {
        let mut m = OtaSlotModel::new();
        m.begin_download();
        assert!(m.finish_download(true));
        assert!(m.verify(true));
        assert!(m.commit());
        m.reboot();

        assert!(!m.health_check(false));
        assert_eq!(m.state, OtaState::RolledBack);
        assert!(m.rolled_back);
        assert!(!m.boot_healthy);
        // Reverted: bootloader falls back to slot A even though B was committed.
        assert_eq!(m.active_slot, SLOT_A);
        assert_eq!(m.boot_slot(), SLOT_A);
    }

    /// Interrupted write (power cut during download): the partial image is
    /// discarded, the update aborts, and slot A remains active.
    #[test]
    fn interrupted_write_rolls_back_to_slot_a() {
        let mut m = OtaSlotModel::new();
        m.begin_download();
        assert_eq!(m.target_slot, SLOT_B);

        assert!(!m.finish_download(false));
        assert_eq!(m.state, OtaState::RolledBack);
        assert!(m.rolled_back);
        assert!(!m.image_written);

        // Verify/commit are no-ops after the aborted download.
        assert!(!m.verify(true));
        assert!(!m.commit());
        assert_eq!(m.active_slot, SLOT_A);
        assert_eq!(m.boot_slot(), SLOT_A);
    }

    /// A committed, healthy update is permanent: a later rollback is ignored.
    #[test]
    fn healthy_update_is_not_undone_by_rollback() {
        let mut m = OtaSlotModel::new();
        m.begin_download();
        assert!(m.finish_download(true));
        assert!(m.verify(true));
        assert!(m.commit());
        m.reboot();
        assert!(m.health_check(true));
        assert_eq!(m.active_slot, SLOT_B);

        m.rollback();
        assert_eq!(m.state, OtaState::Healthy);
        assert_eq!(m.active_slot, SLOT_B);
        assert_eq!(m.boot_slot(), SLOT_B);
    }

    /// commit() refuses to arm a slot when the image never finished writing,
    /// even if a (stale) crc_ok flag is set — defence in depth.
    #[test]
    fn commit_requires_written_image() {
        let mut m = OtaSlotModel::new();
        m.begin_download();
        // Skip finish_download: image_written stays false.
        assert!(m.verify(true));
        assert!(!m.commit());
        assert_eq!(m.state, OtaState::RolledBack);
        assert_eq!(m.active_slot, SLOT_A);
    }

    /// Out-of-order calls are rejected without corrupting state.
    #[test]
    fn transitions_are_guarded_by_state() {
        let mut m = OtaSlotModel::new();
        // Can't verify/commit/reboot/health-check from IDLE.
        assert!(!m.verify(true));
        assert!(!m.commit());
        m.reboot();
        assert!(!m.health_check(true));
        assert_eq!(m.state, OtaState::Idle);
        assert_eq!(m.boot_slot(), SLOT_A);
    }

    #[test]
    fn crc32_matches_standard_check_vector() {
        // The canonical CRC-32/ISO-HDLC check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn metadata_record_crc_round_trip() {
        let mut r = MetaRecord {
            active_slot: SLOT_A,
            target_slot: SLOT_B,
            state: OtaState::CommitPending as u8,
            committed: 1,
            generation: 7,
            image_length: 4096,
            image_crc32: 0xDEAD_BEEF,
            ..Default::default()
        };
        r.finalize();
        assert!(r.is_valid(), "finalized record must validate");
        assert_eq!(r.magic, OTA_META_MAGIC);
        assert_eq!(r.to_bytes().len(), OTA_META_RECORD_SIZE);

        // Any tampering breaks the record CRC.
        let mut bad = r;
        bad.target_slot = SLOT_A;
        assert!(!bad.is_valid(), "tampered record must be invalid");
    }

    #[test]
    fn select_record_picks_highest_valid_generation() {
        let mut a = MetaRecord {
            generation: 3,
            state: OtaState::Healthy as u8,
            ..Default::default()
        };
        let mut b = MetaRecord {
            generation: 5,
            state: OtaState::Healthy as u8,
            ..Default::default()
        };
        a.finalize();
        b.finalize();
        assert_eq!(select_record(&a, &b).unwrap().generation, 5);

        // One invalid -> the valid one wins regardless of generation.
        let mut invalid = b;
        invalid.record_crc32 ^= 0xFFFF;
        assert_eq!(select_record(&a, &invalid).unwrap().generation, 3);

        // Neither valid -> None (caller initialises slot A known-good).
        let mut also_bad = a;
        also_bad.magic = 0;
        assert!(select_record(&also_bad, &invalid).is_none());
    }

    #[test]
    fn abort_records_reason_and_rolls_back() {
        let mut m = OtaSlotModel::new();
        m.begin_download();
        m.abort(0x2A);
        assert_eq!(m.state, OtaState::RolledBack);
        assert!(m.rolled_back);
        assert_eq!(m.abort_reason, 0x2A);
        assert_eq!(m.active_slot, SLOT_A);

        // A committed healthy update is never aborted.
        let mut h = OtaSlotModel::new();
        h.begin_download();
        h.finish_download(true);
        h.verify(true);
        h.commit();
        h.reboot();
        h.health_check(true);
        h.abort(0x99);
        assert_eq!(h.state, OtaState::Healthy);
        assert_eq!(h.abort_reason, 0, "healthy update keeps no abort reason");
    }

    #[test]
    fn recover_after_reset_reconstructs_committed_target() {
        let mut r = MetaRecord {
            active_slot: SLOT_A,
            target_slot: SLOT_B,
            state: OtaState::CommitPending as u8,
            committed: 1,
            ..Default::default()
        };
        r.finalize();
        let mut m = OtaSlotModel::new();
        m.recover_after_reset(&r);
        assert_eq!(m.state, OtaState::CommitPending);
        assert_eq!(m.target_slot, SLOT_B);
        assert!(m.committed);
        assert!(
            m.image_written && m.crc_ok,
            "committed target implies written+verified"
        );

        // A rolled-back record recovers as rolled back.
        let mut rb = MetaRecord {
            active_slot: SLOT_A,
            target_slot: SLOT_A,
            state: OtaState::RolledBack as u8,
            abort_reason: 3,
            ..Default::default()
        };
        rb.finalize();
        let mut m2 = OtaSlotModel::new();
        m2.recover_after_reset(&rb);
        assert_eq!(m2.state, OtaState::RolledBack);
        assert!(m2.rolled_back);
        assert_eq!(m2.abort_reason, 3);
    }
}
