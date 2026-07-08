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
}
