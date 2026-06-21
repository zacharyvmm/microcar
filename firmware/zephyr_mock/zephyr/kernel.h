// zephyr/kernel.h — Minimal Zephyr kernel API for standalone simulation.
//
// Provides the subset of Zephyr's <zephyr/kernel.h> that the microcar
// dashboard firmware uses.  All z_impl_* functions are implemented in
// kernel_impl.c and bridge to sim_zephyr_abi.h / sim_abi.h.

#ifndef ZEPHYR_MOCK_KERNEL_H
#define ZEPHYR_MOCK_KERNEL_H

#include <stdint.h>
#include <stddef.h>

// ── Include generated syscalls (from sim-zephyr-port config) ───────
// The generated syscall wrappers (k_thread_create, k_sleep, etc.) are
// in zephyr/syscalls/kernel.h.  We include them here so <zephyr/kernel.h>
// pulls in the syscall declarations automatically, matching the real
// Zephyr include structure.
//
// Because our include path order puts firmware/zephyr_mock/ BEFORE
// sim-zephyr-port/config/, this file (zephyr/kernel.h) is found first,
// and it delegates to the generated syscalls in zephyr/syscalls/kernel.h.
//
// HOWEVER: the generated syscalls delegate to z_impl_* functions.
// We provide our own mock z_impl_* implementations in kernel_impl.c
// that bridge to the sim ABI.

#ifdef __cplusplus
extern "C" {
#endif

// ── Basic types ─────────────────────────────────────────────────────
//
// These match the real Zephyr types well enough for the dashboard
// firmware to compile.

typedef void (*k_thread_entry_t)(void *, void *, void *);

/// Opaque thread struct — the real TCB is managed by sim-zephyr-port.
struct k_thread {
    int _dummy;
};

/// Thread stack type — essentially a char buffer.
typedef char k_thread_stack_t;

/// Thread ID — pointer to k_thread.
typedef struct k_thread *k_tid_t;

// ── Timeout type ────────────────────────────────────────────────────

typedef struct {
    int32_t ticks; // negative = relative timeout in ticks
} k_timeout_t;

// ── Timeout constructors ────────────────────────────────────────────

// K_MSEC: convert milliseconds to a relative timeout.
// In our model, 1 ms = 1000 µs = 1000 ticks (1 tick = 1 µs).
// We store negative ms so z_impl_k_sleep can multiply by 1000.
#define K_MSEC(ms)      ((k_timeout_t){ .ticks = -(int32_t)(ms) })

// K_FOREVER: used as sentinel value in z_impl_k_sleep.
#define K_FOREVER        ((k_timeout_t){ .ticks = 0x7FFFFFFF })

// K_NO_WAIT: zero delay.
#define K_NO_WAIT        ((k_timeout_t){ .ticks = 0 })

// K_TICKS: absolute or relative ticks.
#define K_TICKS(t)       ((k_timeout_t){ .ticks = (int32_t)(t) })

// ── Stack allocation macros ─────────────────────────────────────────

// K_THREAD_STACK_DEFINE: allocate a stack buffer.
#define K_THREAD_STACK_DEFINE(name, size) \
    static k_thread_stack_t name[size]

// K_THREAD_STACK_SIZEOF: size of a stack in bytes.
#define K_THREAD_STACK_SIZEOF(stack) \
    (sizeof(stack))

// ── z_impl_ declarations (implemented in kernel_impl.c) ─────────────
//
// The generated syscall wrappers in zephyr/syscalls/kernel.h call these.
// We redeclare them here so the linker finds our mock implementations.

extern k_tid_t z_impl_k_thread_create(struct k_thread *new_thread,
                                      k_thread_stack_t *stack,
                                      size_t stack_size,
                                      k_thread_entry_t entry,
                                      void *p1, void *p2, void *p3,
                                      int prio, uint32_t options,
                                      k_timeout_t delay);

extern int32_t z_impl_k_sleep(k_timeout_t timeout);

extern void z_impl_k_yield(void);

// ── Inline syscall wrappers ─────────────────────────────────────────
//
// These replicate the generated syscall wrappers from
// zephyr/syscalls/kernel.h.  We define them here so the dashboard
// firmware can call k_thread_create() and k_sleep() directly.

static inline k_tid_t k_thread_create(struct k_thread *new_thread,
                                      k_thread_stack_t *stack,
                                      size_t stack_size,
                                      k_thread_entry_t entry,
                                      void *p1, void *p2, void *p3,
                                      int prio, uint32_t options,
                                      k_timeout_t delay)
{
    return z_impl_k_thread_create(new_thread, stack, stack_size,
                                  entry, p1, p2, p3, prio, options, delay);
}

static inline int32_t k_sleep(k_timeout_t timeout)
{
    return z_impl_k_sleep(timeout);
}

static inline void k_yield(void)
{
    z_impl_k_yield();
}

// ── Semaphore stubs ─────────────────────────────────────────────────
// Not used by the dashboard firmware but declared for compatibility.

struct k_sem {
    int _dummy;
};

// Inline stubs (no-ops for standalone mode).
static inline int k_sem_init(struct k_sem *sem, unsigned int initial_count,
                             unsigned int limit) {
    (void)sem; (void)initial_count; (void)limit;
    return 0;
}
static inline void k_sem_give(struct k_sem *sem) { (void)sem; }
static inline int k_sem_take(struct k_sem *sem, k_timeout_t timeout) {
    (void)sem; (void)timeout;
    return 0;
}

#ifdef __cplusplus
}
#endif

#endif // ZEPHYR_MOCK_KERNEL_H
