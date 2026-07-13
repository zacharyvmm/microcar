//! Minimal FreeRTOS boot test — single machine, no World.
//!
//! NOTE: This example requires the microcar_boot symbol which was removed
//! when the monolithic firmware was migrated to per-ECU boot functions.
//! Re-enable after B2 C-global migration completes.

fn main() {
    eprintln!("boot_test example requires microcar_boot symbol — not yet available");
    eprintln!("See UNBLOCKING.md §B2 for migration plan");
}

