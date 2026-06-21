// kernel_impl.c — Mock Zephyr kernel implementations bridging to sim ABI.
//
// Provides z_impl_* functions that the generated Zephyr syscall wrappers
// (zephyr/syscalls/kernel.h) call.  These delegate to sim_zephyr_abi.h
// and sim_abi.h from sim-zephyr-port.

#include "zephyr/kernel.h"     // our mock kernel types
#include "sim_zephyr_abi.h"    // from sim-zephyr-port/c/
#include "sim_abi.h"           // from sim-ffi/include/
#include <stdint.h>
#include <stddef.h>

// ── Thread creation ─────────────────────────────────────────────────

k_tid_t z_impl_k_thread_create(struct k_thread *new_thread,
                                k_thread_stack_t *stack,
                                size_t stack_size,
                                k_thread_entry_t entry,
                                void *p1, void *p2, void *p3,
                                int prio, uint32_t options,
                                k_timeout_t delay)
{
    (void)new_thread;
    (void)stack;
    (void)options;
    (void)delay;

    // Register the thread with the Rust fiber runtime.
    zephyr_tid_t tid = sim_zephyr_register_thread(
        "zephyr_task",         // name
        entry,                 // Zephyr 3-arg entry
        p1, p2, p3,
        (uint32_t)stack_size,  // stack size in bytes
        (uint32_t)prio
    );

    // Return the TID as a k_tid_t (pointer-sized).
    return (k_tid_t)(uintptr_t)tid;
}

// ── Sleep ───────────────────────────────────────────────────────────

int32_t z_impl_k_sleep(k_timeout_t timeout)
{
    if (timeout.ticks == 0x7FFFFFFF) {
        // K_FOREVER: sleep indefinitely
        sim_task_delay_until(UINT64_MAX);
    } else if (timeout.ticks < 0) {
        // Relative timeout (e.g., K_MSEC): sleep for -ticks milliseconds.
        // Each millisecond = 1000 microseconds = 1000 ticks (1 tick = 1 µs).
        uint32_t us = (uint32_t)(-timeout.ticks) * 1000;
        uint64_t now = sim_now_ticks();
        sim_task_delay_until(now + (uint64_t)us);
    } else if (timeout.ticks > 0) {
        // Absolute timeout in ticks.
        sim_task_delay_until((uint64_t)timeout.ticks);
    }
    // timeout.ticks == 0: K_NO_WAIT — no delay.
    return 0;
}

// ── Yield ───────────────────────────────────────────────────────────

void z_impl_k_yield(void)
{
    sim_port_yield();
}
