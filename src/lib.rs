//! microcar — costar dogfood firmware bridge
//!
//! [`MicrocarFirmware`] implements the [`Firmware`](sim_world::firmware::Firmware)
//! trait.  Each microcar ECU gets its own firmware instance.
//!
//! In `init()`, the firmware activates the machine's SimGlobal and calls
//! `microcar_boot()` from C to create FreeRTOS tasks.  In `step()`, it
//! calls `sim_scheduler_tick()` to advance the FreeRTOS scheduler by one
//! cycle on the machine's fiber pool.

use sim_world::firmware::Firmware;
use sim_world::Machine;
use sim_core::Tick;

// C ABI functions from the compiled firmware / sim-ffi.
extern "C" {
    fn microcar_boot();
    fn microcar_boot_gateway();
    fn microcar_boot_powertrain();
    fn microcar_boot_bms();
    fn microcar_boot_dashboard();
    fn sim_scheduler_tick() -> u32;
}

/// Firmware for a single microcar ECU.
pub struct MicrocarFirmware {
    pub name: String,
    booted: bool,
}

impl MicrocarFirmware {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            booted: false,
        }
    }
}

impl Firmware for MicrocarFirmware {
    fn init(&mut self, machine: &mut Machine) {
        let _guard = machine.activate();

        // Select the right boot function based on the machine name.
        // Each machine runs only its designated ECU firmware.
        unsafe {
            if self.name.starts_with("gateway") {
                microcar_boot_gateway();
            } else if self.name.starts_with("powertrain") {
                microcar_boot_powertrain();
            } else if self.name.starts_with("bms") {
                microcar_boot_bms();
            } else if self.name.starts_with("dashboard") {
                microcar_boot_dashboard();
            } else {
                // Fallback: boot all 4 ECUs for unknown machine types.
                microcar_boot();
            }
        }

        // Flush thread-local trace events into this machine's SimGlobal.
        sim_ffi::flush_trace();

        self.booted = true;
    }

    fn step(&mut self, _now: Tick, machine: &mut Machine) {
        if !self.booted {
            return;
        }

        // Activate this machine's SimGlobal for the scheduler tick.
        let _guard = machine.activate();

        // Advance the FreeRTOS scheduler by one cycle.
        // Returns 1 if more work remains, 0 if all tasks are done.
        // sim_scheduler_tick() internally calls flush_trace() after each
        // cycle, so firmware trace events are attributed to this machine.
        unsafe {
            sim_scheduler_tick();
        }

        // Flush again in case any trace events remain (e.g., from the
        // tickless idle path which may not have flushed).
        sim_ffi::flush_trace();
    }
}
