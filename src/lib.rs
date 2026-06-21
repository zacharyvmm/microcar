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
        // Activate this machine's SimGlobal so C ABI functions
        // use its task pool and trace sink.
        let _guard = machine.activate();

        // Create FreeRTOS tasks (gateway, powertrain, bms, dashboard).
        // The C function microcar_boot() calls xTaskCreate for each.
        unsafe {
            microcar_boot();
        }
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
        unsafe {
            sim_scheduler_tick();
        }
    }
}
