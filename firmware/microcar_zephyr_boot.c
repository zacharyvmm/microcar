// microcar_zephyr_boot.c — Boot entry for Zephyr-based dashboard ECU.
//
// This file is compiled when the "zephyr" feature is enabled.  It
// registers the Zephyr dashboard thread with the Rust fiber runtime
// via sim_zephyr_register_thread().

#include "sim_zephyr_abi.h"
#include "sim_abi.h"
#include <stddef.h>
#include <stdint.h>

// ── Dashboard thread entry (from dashboard_ecu_zephyr/src/main.c) ──
//
// dashboard_thread_entry has the Zephyr 3-arg signature:
//   void dashboard_thread_entry(void *arg1, void *arg2, void *arg3);
//
// It is declared in main.c, not in a header, so we forward-declare it.
extern void dashboard_thread_entry(void *, void *, void *);

// ── Boot the Zephyr dashboard ──────────────────────────────────────
//
// Called from Rust (MicrocarFirmware::init) to register the dashboard
// thread with the Zephyr fiber runtime on this machine.

void microcar_boot_dashboard_zephyr(void)
{
    sim_zephyr_init();

    sim_zephyr_register_thread(
        "dashboard_zephyr",
        (zephyr_thread_entry_t)(uintptr_t)dashboard_thread_entry,
        NULL, NULL, NULL,   // arg1, arg2, arg3
        4096,               // stack size (bytes)
        1                   // priority
    );

    sim_trace_u32("microcar_zephyr_boot", 1);
}
