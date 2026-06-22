# Scheduler Realism Plan

How to make microcar a better stress test for costar's scheduler.

## Current Architecture Summary

The firmware is real FreeRTOS C running on costar fibers. All 25 scenarios pass with
deterministic golden traces, fleet-of-64 scales, and overnight-soak runs leak-free.

However, the scheduler is driven **tick-by-tick** from Rust via `sim_scheduler_tick()`,
NOT via FreeRTOS's own `vTaskStartScheduler()`. Within each tick, every task runs to
completion via `vTaskDelayUntil` (voluntary cooperative yield). There are no ISR-driven
wakeups, no preemption, no sporadic workloads. Costar sees a perfectly periodic, fully
cooperative workload with zero contention.

## What the Scheduler Is NOT Being Tested For

### 1. Preemption

The `run_one_scheduler_cycle()` function (`sim-ffi/src/lib.rs:400`) sorts runnable
tasks by priority and round-robin distance. This code is exercised, but with purely
cooperative tasks it never faces a genuine scheduling decision — every task voluntarily
yields before the scheduler picks the next one.

A real RTOS workload has higher-priority tasks becoming ready WHILE a lower-priority
task is executing. The scheduler must decide whether to preempt.

**Missing**: no timer callback, ISR, or semaphore-give-from-ISR that wakes a
higher-priority blocked task during a lower-priority task's execution.

### 2. Priority Inversion

Mutexes exist (gateway `g_fm_mutex`) but no scenario creates priority inversion:

```
T1 (prio 1): takes mutex
T2 (prio 5): tries to take mutex → blocks
T3 (prio 3): runs, starving T1 → T2 starves indefinitely
```

Without priority inheritance, this is a classic embedded bug. Costar should either
implement inheritance or at least make the starvation visible in traces.

### 3. Bursty / Sporadic Activation

Every task is perfectly periodic. Real embedded workloads include:
- Burst processing (50 CAN frames in 5ms)
- Event-driven activation (block on semaphore until message arrives)
- Variable execution time per activation

### 4. Queue Backpressure / Overflow

`g_hb_queue` is depth 16, but heartbeats arrive at 100ms intervals. The queue never
fills. Real systems have producers that outrun consumers, testing overflow behavior.

### 5. Runtime Task Lifecycle

BMS does `xTaskCreateStatic` + `vTaskDelete` for calibration — good but limited.
Missing:
- Rapid create/delete cycling (50+ cycles in 10s)
- Delete a task that holds a mutex
- `vTaskSuspend` / `vTaskResume`
- `vTaskPrioritySet` at runtime

### 6. Deeper IPC Patterns

Gateway uses queue + mutex + event group + task notifications. Good start.
Missing within a single ECU:
- Stream buffers / message buffers
- Producer-consumer with bounded buffer and actual backpressure
- Rendezvous (two tasks synchronizing)
- Readers-writers pattern

### 7. Stack Overflow and Resource Exhaustion

No scenario tests:
- Stack overflow (`uxTaskGetStackHighWaterMark`)
- Heap exhaustion (pvPortMalloc returning NULL)
- Timer queue overflow
- Critical section nesting depth

### 8. Mixed RTOS Depth

The Zephyr dashboard uses mock headers (`firmware/zephyr_mock/zephyr/kernel.h`).
It reuses dashboard business logic but the Zephyr scheduler path is superficial —
no real Zephyr kernel scheduling decisions are tested alongside FreeRTOS.

---

## Implementation Plan (ordered by impact on scheduler testing)

### Phase R1 — ISR-Driven Preemption (HIGHEST IMPACT)

Add a `can_frame_processor` task (priority 5) to the gateway that blocks on a
counting semaphore. When `heartbeat_rx` (priority 4) receives a CAN frame, it
gives the semaphore, which should preempt whatever lower-priority task was running.

```
Gateway ECU task set after R1:
  can_frame_processor  prio 5  — blocks on semaphore, processes frames on wake
  heartbeat_rx         prio 4  — drains CAN RX, gives semaphore per frame
  gateway_main         prio 3  — mode/heartbeat loop
  fault_aggregator     prio 2  — low-rate fault stats
```

**Costar features exercised**: priority-ordered preemption, semaphore wakeup
from within a fiber, scheduler's runnable-task selection under contention.

**New scenario**: `can_burst_preempt.toml` — inject 20 CAN frames at t=500ms.
Verify `can_frame_processor` trace events interleave correctly (preempting
lower-priority tasks).

**Effort**: ~150 lines of C, 1 new scenario TOML, 1 golden trace regeneration.

### Phase R2 — Priority Inversion Scenario

Add a three-task scenario (new ECU or modified gateway) that deliberately
creates priority inversion:

```
low_task  (prio 1): takes mutex, starts long computation (200ms)
med_task  (prio 3): busy-loops with periodic trace
high_task (prio 5): tries to take mutex → blocks
```

**Costar features exercised**: mutex ownership tracking, priority inheritance
(or verified lack thereof), scheduler's handling of mutex-blocked tasks in
the runnable selection.

**New file**: `firmware/priority_inversion_demo/src/main.c` (single-file ECU)

**New scenario**: `priority_inversion.toml`

**Effort**: ~200 lines of C, 1 new scenario TOML.

### Phase R3 — Bursty Workload Patterns

#### R3a: Frame burst scenario

Modify `heartbeat_rx` (or add a burst test ECU) to handle a burst of CAN frames.
The scenario injects 50 frames at t=500ms over 5ms virtual time.

**What to verify**:
- Queue fills, overflows (or blocks producer)
- Higher-priority task preempts to drain queue
- No frames dropped due to scheduler starvation

#### R3b: Event-driven task pattern

Add a task that blocks on `xSemaphoreTake(pdMS_TO_TICKS(portMAX_DELAY))` until
a CAN frame arrives. This exercises indefinite blocking and wake-from-block
paths in the scheduler.

**New scenario**: `event_driven_wake.toml`

**Effort**: ~100 lines of C changes (modify gateway), 1 new scenario.

### Phase R4 — Runtime Lifecycle Stress

#### R4a: Rapid create/delete

Create a stress ECU that cycles through 100 create→run→delete operations.
Each worker task runs for a random (deterministic-seeded) duration, then
deletes itself. Verify fiber count returns to baseline.

#### R4b: Suspend / resume / priority change

Add a scenario where the dashboard's `display_update` task is suspended
at t=500ms (stops tracing), then resumed at t=1500ms (resumes tracing).
Change `fault_aggregator` priority from 2 to 5 at t=1000ms and verify
it starts preempting other tasks.

**Effort**: ~300 lines of C, 2 new scenarios.

### Phase R5 — IPC Depth

Within the gateway ECU, add:

1. **Stream buffer** from heartbeat_rx → gateway_main for raw heartbeat data
   (replaces the current queue-based approach)
2. **Message buffer** for variable-length fault reports from fault_aggregator
   → gateway_main
3. **Task notification as counting semaphore** — increment/decrement pattern
   for tracking available CAN TX mailbox slots (similar to powertrain's
   counting semaphore but via task notifications)

**Effort**: ~200 lines of C changes in gateway.

### Phase R6 — Resource Exhaustion

#### R6a: Stack overflow

Add a deliberately recursive function to `logger` task in powertrain.
Register the stack overflow hook (`vApplicationStackOverflowHook`).
Verify costar's stack checking detects it and traces `Fatal(StackOverflow)`.

#### R6b: Heap exhaustion

Configure a deliberately small heap (`configTOTAL_HEAP_SIZE`). Create
tasks until `pvPortMalloc` returns NULL. Verify the malloc failed hook
fires and the system degrades gracefully.

**Effort**: ~150 lines of C, 2 new scenarios.

### Phase R7 — vTaskStartScheduler Integration (ARCHITECTURAL)

Currently `microcar_coordinator.c` creates tasks but never calls
`vTaskStartScheduler()`. The Rust `sim_scheduler_tick()` loop owns
task selection.

Add a parallel path: `microcar_boot_scheduler()` that calls
`vTaskStartScheduler()` → `xPortStartScheduler()` → the real FreeRTOS
idle loop runs on a fiber. This exercises:

- FreeRTOS idle task
- Tick hook (`vApplicationTickHook`)
- Timer daemon task scheduling
- The RTOS's own task selection (vs Rust's manual selection)

This should be a separate scenario (`freertos_scheduler_boot.toml`) that
runs alongside the existing tick-mode scenarios. Both paths are valuable
for different reasons.

**Effort**: ~300 lines of Rust (new Firmware adapter), ~100 lines of C,
careful testing to avoid breaking existing scenarios.

### Phase R8 — Mixed RTOS Depth

The Zephyr dashboard (`firmware/dashboard_ecu_zephyr/`) currently uses
mock Zephyr headers. It links against standalone Zephyr test symbols
(`sim_zephyr_scheduler_tick()`) which is a thin wrapper.

For real mixed-RTOS testing:

1. **Real Zephyr kernel compilation**: Use the `zephyr_real` feature
   with `ZEPHYR_BASE` to compile the dashboard against the actual
   Zephyr v4.1.0 kernel (cc crate path, already working in costar).

2. **CAN bridge on Zephyr side**: The Zephyr dashboard should receive
   real CAN frames from FreeRTOS ECUs via the World CanBus, not just
   trace mock data.

3. **Different scheduling semantics**: Zephyr's cooperative scheduling
   (CONFIG_COOP_ENABLED) vs FreeRTOS's preemptive scheduling on the
   same CAN bus exercises the World drain loop's RTOS-agnostic
   time advancement.

**Effort**: ~200 lines of C changes, 1 new scenario, verify against
existing golden trace.

### Phase R9 — Deadline Monitoring

Add a `deadline_monitor` task to the powertrain that:

1. Records expected wake time via `vTaskDelayUntil`
2. Measures actual wake time via `xTaskGetTickCount()`
3. Computes jitter = |actual - expected|
4. If jitter > 2 ticks, traces `deadline_miss` with the jitter value

This makes scheduler performance regressions immediately visible in
trace output — if a costar change introduces timing drift, golden
trace comparison catches it.

**Effort**: ~80 lines of C in powertrain.

---

## Summary Table

| Phase | Impact | Effort | What It Tests |
|-------|--------|--------|---------------|
| **R1** ISR preemption | ⭐⭐⭐⭐⭐ | ~150L C | Priority scheduling, semaphore wakeup, preemption |
| **R2** Priority inversion | ⭐⭐⭐⭐ | ~200L C | Mutex ownership, priority inheritance, starvation |
| **R3** Bursty workloads | ⭐⭐⭐ | ~100L C | Queue overflow, burst processing, event-driven wake |
| **R4** Lifecycle stress | ⭐⭐⭐ | ~300L C | Create/delete/suspend/resume, priority change |
| **R5** IPC depth | ⭐⭐ | ~200L C | Stream buffers, message buffers, notification patterns |
| **R6** Resource exhaustion | ⭐⭐⭐ | ~150L C | Stack overflow, heap exhaustion, error hooks |
| **R7** vTaskStartScheduler | ⭐⭐⭐⭐ | ~400L Rust+C | Idle task, tick hook, timer daemon, RTOS-owned scheduling |
| **R8** Mixed RTOS depth | ⭐⭐⭐ | ~200L C | Zephyr+FreeRTOS on same bus, different scheduling semantics |
| **R9** Deadline monitoring | ⭐⭐ | ~80L C | Jitter measurement, regression detection in CI |

### Recommended Order

R1 → R2 → R3 → R7 → R4 → R6 → R5 → R8 → R9

R1+R2 give the biggest bang: preemption and priority inversion are the
two scheduling patterns that every RTOS project cares about and that are
currently completely untested. R3 adds workload realism. R7 is the
architectural gap (tick-mode vs full scheduler). The rest are depth.

---

## What NOT To Do (for scheduler testing)

- More ECUs beyond 8 — scaling is already tested by `fleet_of_64`
- More plant physics — doesn't exercise scheduler
- Longer scenarios — `overnight_8hour` already validates
- Real CAN arbitration — peripheral, not scheduler
- Graphical dashboard or UI — zero scheduler value
- Real battery chemistry or motor physics — zero scheduler value
- Bootloader / OTA / secure boot — out of scope entirely

---

*Document generated 2026-06-22 by Hermes Agent, based on analysis of costar @
`/home/zmm/projects/costar` and microcar @ `/home/zmm/projects/microcar`*
