# Microcar Dogfood Unblocking Plan

Last reviewed: 2026-07-10

docs/costar_microcar_dogfood_plan.md is the product goal and docs/BLOCKERS.md
is the status record. This file is the implementation brief for the remaining
work. It chooses durable designs instead of adding more trace-only fixtures
around current simulator limitations.

Starting point: the M12 diagnostics safety lane, M13 charging safety lane, M14
gRPC-surface proof, M15-M18 OTA fixtures, and four debug-gym corpus cases are
already delivered. Preserve them as regression coverage. The remaining corpus
work is BMS stale sensor, dashboard missed warning, and telematics partial
write; the remaining OTA work is the decision-gated real path described below.

## What Unblocked Means

A lane is not unblocked merely because it emits the desired trace label. A
completed remaining lane must have all of these properties:

1. The producer, transport/device layer, consumer, and observable result all
   execute independently in the simulation.
2. The harness asserts semantic state plus relevant transport/device evidence,
   not only a firmware-local trace hook.
3. Default vehicle scenarios and human golden traces remain byte-identical.
   New behavior is enabled by new scenarios or explicit configuration.
4. The lane is deterministic across repeated runs. Host-connected tests use
   timeouts and transcripts, not golden traces.
5. Failures have useful Trace v2, typed-state, or device-inspection evidence.

The current gateway_diag_*, gateway_charging, and gateway_ota* variants are
useful regression fixtures. Keep them green during the migration, but do not
extend them as the final implementation of diagnostics, charging, or OTA. Their
direct scripts bypass the path that this dogfood project is meant to exercise.

## Decisions And Dependency Order

| Stage | Deliverable | Why first | Unlocks |
|---|---|---|---|
| P0a | Per-machine execution/device context in costar | ECUs share thread-local device state, including CAN controller 0. | Real CAN, gRPC isolation, network isolation. |
| P0b | Receiver-correct CAN and firmware-instance state | A frame must reach its intended ECU before bus-backed claims are credible. | Diagnostics, BMS/dashboard seeds, charging, OTA control. |
| P1 | Restartable machine and persistent-storage boundary | Current reboot replaces a machine without restoring firmware. | Gateway-reset OTA fault and real boot recovery. |
| P2 | Protocol-backed scenario stimuli | Prevents tests from forcing the vehicle state under test. | Charging/OTA mode gates and deferred toml_zoo cases. |
| P3 | Real diagnostics, charging, and OTA slices | Delivers the EV behavior in the plan. | Remaining OTA matrix and two debug seeds. |
| P4 | Dashboard/display and telematics slices | Both need P0 isolation. | Cockpit content and final debug seed. |
| P5 | Typed breakpoints and nightly composition | These need real consumers first. | Product debugging and scale confidence. |

P0 is a narrow vertical slice of the larger per-session refactor, not a
permission for a speculative rewrite. Do not call it complete until two
in-process worlds can each run an ECU using device ID 0 without observing the
other world.

## Implementation Review: M23-M27 Remediation Gate

The M23-M27 costar commits provide useful migration primitives, but they do
**not** yet satisfy the P0a, P0b, or P1 contracts in this document. Treat them
as staged implementation, not as completed infrastructure. In particular, do
not claim a bus-backed diagnostics, charging, OTA, cockpit, or reboot lane is
real until this gate is closed.

### Defects found in the staged implementation

1. `DeviceBank::activate` exposes a safe guard backed by a thread-local raw
   pointer. Dropping outer and inner guards out of LIFO order, or forgetting a
   guard, can leave the active pointer dangling. `with_bank` then dereferences
   that pointer in safe code. The analogous `SimGlobal` raw-pointer guard must
   be audited at the same time; do not extend an unsafe lifetime pattern into a
   unified execution context.
2. `Machine` constructs a `Simulator` with no owned device bank, and no
   production `World` or microcar path calls `enable_owned_devices`. gRPC board
   configuration, touch, display streaming, and inspection also call the
   `sim_devices` fallback registry directly. The staged banks therefore prove
   only standalone unit isolation, not World/session isolation.
3. The P0b inbox stages and drains `with_can_mut(0)` outside the receiver
   machine's active context. Enabling an owned bank would make World write the
   default bank while firmware reads its owned bank. In addition,
   `Machine::advance_to` can call `Firmware::step` after the World bridge has
   run; CAN TX/RX from that invocation is not staged or drained in the correct
   machine context and can be attributed to a later machine.
4. The microcar binary attaches firmware with `load_firmware`, not a restart
   factory. A `downtime_ms` reboot therefore creates a bare machine and emits a
   boot marker without restoring firmware. Replacing a machine with
   `Machine::with_defaults` also loses its RTOS and simulator configuration.
5. Bus delivery currently queues frames for stopped machines. Those frames
   survive downtime and are staged after boot, contradicting the reboot
   contract below.

### Primary implementation surfaces

| Gate | Primary files | Required ownership boundary |
|---|---|---|
| B0 context safety | `costar/crates/sim-devices/src/bank.rs`; `costar/crates/sim-ffi/src/lib.rs` | Active-context lifetime and restoration. |
| B1 Machine/session devices | `costar/crates/sim-ffi/src/simulator.rs`; `costar/crates/sim-world/src/machine.rs`; `costar/crates/sim-world/src/board.rs`; `costar/crates/sim-grpc/src/server.rs` | Device provision, inspection, touch, and display belong to the selected machine in one session World. |
| B2 CAN execution | `costar/crates/sim-world/src/world.rs`; `costar/crates/sim-world/src/machine.rs` | Firmware invocation, RX staging, and TX drain happen in one active-machine sequence. |
| B3 restart/product wiring | `costar/crates/sim-world/src/world.rs`; `costar/crates/sim-world/src/machine.rs`; `microcar/src/main.rs`; reboot scenarios | Immutable machine specification survives; volatile runtime resets; the microcar factory is actually used. |

### Required implementation sequence

1. **Make active-context lifetime safe before using it in World.** Prefer a
   closure-scoped activation API that cannot be forgotten or dropped out of
   order. If a guard remains necessary, its active state must retain a valid
   owner for both the current and restored contexts; a raw pointer plus
   `PhantomData` is insufficient. Keep the fallback bank only for legacy
   single-simulator tests during migration, not as a production ownership path.
2. **Make each Machine own and provision its DeviceBank.** Construct that bank
   as part of machine creation and provide an explicit machine-context helper
   for device configuration and inspection. Board/device assignment must name
   the target machine; a World or gRPC operation outside firmware execution
   must select and activate that machine explicitly. Do not repair this with a
   session-keyed global registry.
3. **Centralize all firmware execution and CAN bridging in World.** There must
   be one code path that, while the sender/receiver machine context is active,
   stages that machine's RX inbox, invokes firmware, drains that machine's TX
   queue, and restores unconsumed RX. Remove or refactor the extra
   `Firmware::step` call in `Machine::advance_to` so it cannot bypass this
   sequence. Preserve World-owned bus routing, correlation IDs, sender
   exclusion, and bridge loop prevention.
4. **Bind gRPC to the session World and machine.** ConfigureBoard, touch,
   display streaming, and InspectDevices must access the selected session's
   machine-owned bank while that World is locked. A request for one session
   must never overwrite or inspect controller/display ID 0 in another session.
5. **Finish restart semantics before adding the OTA reset lane.** Store an
   immutable machine specification (identity, name, RTOS, simulator config,
   board/device assignment, and firmware factory) and use it to recreate the
   volatile runtime. Preserve persistent flash/EEPROM, reset volatile devices
   and guest/C state, and keep exactly one pending boot per machine. Drop bus
   deliveries to a stopped machine. At `boot_at`, transition the machine to
   running before processing bus arrivals at that timestamp, so arrivals before
   `boot_at` are dropped and arrivals at or after it are normal post-boot input.
6. **Wire the product consumer.** In `microcar/src/main.rs`, build factories
   from immutable scenario firmware metadata for FreeRTOS and Zephyr machines
   and attach with `load_firmware_from_factory`. Add a real gateway reset
   scenario with nonzero `downtime_ms`; a reset trace marker alone is not
   evidence of reboot recovery.
7. **Only then continue the broader P0 migration.** Move time/task identity,
   network state, and C firmware instance state in separate verified slices as
   specified below. Do not use the successful device-only tests to claim
   duplicate ECU types or concurrent sessions are isolated.

### Mandatory remediation tests

- A context-lifetime regression proves nested contexts restore correctly and
  the public API cannot leave a dangling active context through ordinary safe
  control flow.
- Two actual `World`s on one thread, each with firmware and CAN controller 0,
  can interleave sends and receives without leakage. This is not satisfied by
  two standalone `DeviceBank` or `Simulator` tests.
- A sender that emits CAN work from the formerly separate `advance_to` path is
  attributed to that sender and only reaches its attached buses.
- Two concurrent gRPC sessions each configure, touch, stream, and inspect
  controller/display ID 0 without cross-observation.
- A microcar gateway reboot with nonzero downtime creates a new firmware
  instance, re-runs its boot path, resumes its heartbeat, preserves its RTOS
  and persistent storage, and leaves sibling ECUs intact.
- Frames sent before the reset remain in flight as appropriate; frames sent
  while the target is down are dropped and cannot appear after boot.
- Run the two-World, gRPC, CAN, and reboot tests repeatedly (at least 100
  iterations where practical) and retain byte-identical existing golden lanes.

## 1. Per-Machine Execution And Device Ownership (P0a)

### Root cause

Three current facts explain several blockers:

- `sim_devices` accessors fall back to a thread-local default bank because no
  production `Machine` owns and activates its own bank.
- World keeps receiver inboxes, but stages and drains controller 0 through that
  fallback outside the relevant machine context; `Machine::advance_to` also has
  a firmware execution path outside the World bridge.
- sim-grpc configures and inspects those global maps directly in
  crates/sim-grpc/src/server.rs.

The existing active SimGlobal guard is useful, but SIM_NOW,
CURRENT_TASK_ID, device registries, network registries, and some C firmware
globals still escape it. This is why firmware CAN RX is unreliable and why the
cockpit test is intentionally sequential.

### Target ownership model

    World
      MachineRuntime (one per machine)
        Simulator / scheduler / trace sink
        DeviceBank (CAN, timers, IRQ, UART, display, touch, ADC, storage, ...)
        GuestRuntime (current time, task identity, firmware-instance state)
      World-owned buses, links, plant, and session metadata
      World-owned network state where it is not machine-specific

The FFI may keep a thread-local active-execution pointer only as an
RAII-scoped dispatch mechanism while guest code runs. It must point to the
owning MachineRuntime, restore the prior context on every exit path, and never
be the storage location for state.

- FFI reads of time and current task ID resolve through the active context, not
  process-global atomics.
- FFI device calls resolve a device ID inside the active machine DeviceBank.
  Each ECU can continue to use controller ID 0 without collisions.
- World and gRPC operations outside guest execution take an explicit World and
  machine selector. They must not depend on the most recently active machine.
- Host pollers, TAP/TCP bridges, smoltcp state, and display/touch state move
  out of thread-local registries on the same ownership path.

Do not fix CAN by introducing another process-global map keyed by session ID.

### C firmware instance state is included

The C ECU files also contain mutable static state, task handles, stacks, and
TCBs. One linked copy is shared by all instances of an ECU type in the host
process. A Rust-only DeviceBank migration therefore does not make fleets or
concurrent gRPC sessions safe.

Add a small FFI-backed firmware-instance storage API, conceptually:

    mc_instance_state(module_key, size, alignment)
      -> pointer owned by the active machine

Its volatile region belongs to GuestRuntime and is reset by a machine reboot.
Its persistent region is backed by virtual flash. Migrate mutable C state into
per-instance structs in this order:

1. Gateway state, fault manager, queues, semaphores, and selectors.
2. Powertrain controller and task handles.
3. BMS state, calibration state, stacks, and TCBs.
4. Dashboard state, warning/display task state, stacks, and TCBs.
5. New charging, OTA, and telematics state.

Immutable lookup tables may stay static const. Do not use C thread-local storage
as a substitute: sessions can execute on the same worker thread.

### Migration sequence

1. Inventory every thread_local, static device registry, SIM_NOW,
   CURRENT_TASK_ID, active simulator guard, and C mutable global. Assign each
   an owner: machine, world, session, or true process service.
2. Add DeviceBank and explicit accessors in sim-devices. Port one low-risk
   device class plus inspection and add two-world leakage tests.
3. Extend SimulatorActivation into an execution-context guard that activates
   SimGlobal, DeviceBank, time, and task identity together. Test nested
   activation and panic/unwind restoration.
4. Port CAN next, then timers/IRQ, then display/touch/ADC/storage.
5. Port sim-net and the host poller after the device API shape is stable. A
   host bridge belongs to one world/session and is cleaned up on destruction.
6. Migrate microcar C globals before enabling in-process fleet or concurrent
   gRPC tests with duplicate ECU types.
7. Remove production use of legacy global accessors.

### P0a exit tests

- Two worlds on one thread, each with CAN controller 0, enqueue and receive
  different frames without cross-observation.
- Interleaving those worlds in either order produces each solo trace.
- A panic in one active world restores the prior active context and a sibling
  world still runs.
- Board configuration, inspection, touch, and display streaming are
  session-local when two gRPC sessions use the same device IDs.
- Two same-type microcar ECUs have independent task IDs, C state, and queues.
- Existing costar and microcar golden scenarios remain byte-identical after
  every migration slice. Do not batch clock, device, and C-state moves.

## 2. Correct CAN Delivery (P0b)

After P0a, repair CAN as a direct consequence of ownership, not with
receiver-side filtering workarounds.

For each logical send, World must:

1. Drain the sender machine active controller 0 after that firmware steps.
2. Put the frame only on buses to which that sender is attached.
3. Deliver one copy to each eligible receiver on that bus.
4. Inject the copy into that receiver controller 0 inbox.
5. Record one sender CanTx and one receiver CanRx per delivery while preserving
   existing Trace v2 correlation and parent behavior.

The current assumption that firmware can discard unrelated frames is invalid:
an unrelated ECU can consume the shared queue first. Sender exclusion and bridge
loop prevention remain World/bus responsibilities.

Required regression matrix:

- Three firmware ECUs using controller 0: one sender and two receivers. Each
  receiver gets exactly one intended frame.
- A gateway on two buses forwards exactly once with a child correlation ID.
- A passive external sender injected through bus_inject reaches only attached
  receivers.
- A 100-run repetition proves ordering and trace hashes are stable.
- Two gRPC sessions use the same controller IDs concurrently and each
  InspectDevices result is session-local.

This is the gate for replacing the diagnostics scripts with the existing
diagnostics-tool ECU. It is also the gate for claiming OTA reset or BMS fault
paths are end to end.

## 3. Restartable Machines And Persistent State (P1)

The current microcar product path still creates a fresh default Machine without
restoring firmware because it does not register a firmware factory. It is not a
reset primitive suitable for OTA. Fix generic simulator semantics and wire the
product consumer first; do not model a gateway reset with a trace marker.

### Reboot contract

Store a restartable firmware specification/factory in Machine or scenario
construction. A reboot must:

1. Preserve machine identity, bus attachments, board/device assignment, and
   persistent flash/EEPROM.
2. Discard guest RAM, task/fiber state, volatile device registers/mailboxes,
   and per-instance C volatile state.
3. Keep frames already transmitted on the bus in flight, but discard the reset
   target pre-reset receive queue. Frames sent while it is down are not
   delivered retroactively after boot.
4. Recreate original firmware from its factory and call its normal boot path
   after an explicit deterministic downtime_ms interval.
5. Emit structured machine_reset_begin and machine_reset_boot events with
   identity and virtual time.

Add downtime_ms as an optional generic field to the existing machine.reboot
fault. It belongs in costar because it is generic reset semantics. OTA scenarios
should use a nonzero duration and assert the boot boundary.

### Persistence contract

A/B metadata is persistent storage, not a C static local to an OTA script.
Store metadata and image validity in the gateway persistent flash region. The
RAM-side worker may disappear at reset; boot recovery must re-read metadata and
choose the known-good slot or the eligible target deterministically.

Extend common/microcar_ota_slot.c and its Rust mirror with explicit recovery,
for example abort(reason) and recover_after_reset(). Keep it pure and
unit-test every transition before FreeRTOS integration.

### P1 exit tests

- A normal gateway reboot runs its original firmware again and resumes a
  heartbeat after downtime.
- Rebooting one ECU does not reset sibling ECUs or devices.
- The rebooted machine preserves RTOS/configuration and persistent storage while
  discarding only volatile runtime state.
- Frames arriving before `boot_at` are dropped; they cannot be delivered after
  boot. A frame at `boot_at` follows the documented post-boot ordering.
- An uncommitted target is rejected after gateway reset and slot A remains
  active.
- A failed post-update health check rolls back from persistent metadata, not
  surviving RAM.
- Keyframe replay around reset reproduces the same future.

## 4. Scenario Stimuli Without Forced Vehicle State (P2)

Do not add mode = "CHARGING" or mode = "OTA_UPDATE" to scenarios. That would
bypass gateway authority and make safety assertions circular.

The existing generic bus_inject mechanism is enough for the first real vehicle
stimuli. Model external actors as passive named machines attached to the correct
bus, with no firmware, such as evse, update_tool_host, or test_harness. They
supply a real sender identity and use normal World bus delivery.

    [[machine]]
    id = 6
    name = "evse"

    [[bus.node]]
    bus = "vcan_body"
    machine = "evse"

    [[bus_inject]]
    at_ms = 100
    bus = "vcan_body"
    sender = "evse"
    id = 0x620 # example: MC_MSG_EVSE_STATUS
    data = [6, 1, 0, 0]

Protocol IDs and payloads belong in microcar/common/include/microcar_protocol.h;
costar only knows this is a CAN frame. Add microcar semantic validation that:

- the passive sender is attached to the named bus;
- payload byte 0 matches the declared protocol node ID where the protocol uses
  an in-payload source ID;
- event order and values are legal for the relevant vehicle protocol;
- a scenario cannot bypass gateway admission.

Use generic fault for physical failure/reset timing and bus_inject for external
protocol events. Add a new generic scenario field only if it represents a
reusable simulator concept that neither primitive can express.

This solves the deferred mode cases without a schema shortcut:

- charging-while-drive: driver input establishes DRIVE, then EVSE plug and
  handshake frames arrive; gateway/powertrain block torque.
- ota-while-drive: driver input establishes DRIVE, then a real OTA request
  arrives; gateway rejects it.
- ota-while-charging: EVSE establishes CHARGING, then a real OTA request
  arrives; gateway rejects it.

## 5. Real Firmware Vertical Slices

### 5.1 Diagnostics: live BMS data over real CAN

The diagnostics-tool ECU already sends 0x600 requests and decodes 0x601
responses. After P0b it becomes the authoritative path.

1. BMS publishes a compact, sequenced snapshot from data it actually receives
   from the plant. Use a source byte and fixed-point fields that fit in one CAN
   payload. Do not claim ISO-TP or UDS support.
2. Gateway caches the latest valid snapshot with sequence and receive time. It
   answers MC_DIAG_LIVE_BMS only while fresh, otherwise responds
   REJECTED/STALE instead of replaying old data.
3. Keep the two-byte response shape with documented snapshot selectors:
   SOC/temperature, voltage, and current can be separate deterministic requests
   with fixed scaling. The tool assembles the snapshot.
4. The harness asserts tool CanTx, gateway CanRx, gateway CanTx, tool CanRx,
   decoded values, freshness, and Trace v2 causality.

Add a successful live-data scenario and an actual stale-data scenario. Staleness
must be caused by missing/stopped plant-to-BMS data or BMS publication, not a
gateway trace flag. This work is the basis for the BMS stale-sensor seed.

### 5.2 Charging: small closed-loop EV model

Use a compact gateway-owned FSM:

    DISCONNECTED -> PLUG_DETECTED -> HANDSHAKE -> ACTIVE
                                             -> LIMITED
    ACTIVE/LIMITED -> COMPLETE | FAULT

Vehicle mode remains CHARGING while plugged, including COMPLETE. Only unplug
plus normal gateway checks can leave it. The powertrain rule independently
allows torque only in DRIVE/LIMP.

Implementation order:

1. Define EVSE status/request and charge-command messages in the microcar
   protocol. EVSE is an external bus actor; BMS supplies a real charge-current
   limit based on temperature/SOC.
2. Put the transition table in a pure unit-tested charging module. Invalid
   events emit a rejection reason; no state is silently forced.
3. Make the plant a real bus endpoint for actuator commands. It consumes
   powertrain motor commands and gateway/BMS charge commands through an
   explicit plant inbox/environment callback, not by mapping driver throttle
   directly to motor torque as the MVP plant currently does.
4. Extend the fixed-point battery model with signed charge current, deterministic
   SOC increase, temperature evolution, and bounded current limiting.
5. Broadcast mode/charge command through real CAN and have powertrain receive
   mode through its own controller before clamping torque.

Add scenarios in this order: plug/handshake reaches ACTIVE; high temperature
reaches LIMITED; SOC target reaches COMPLETE; critical BMS fault stops charging;
and drive while plugged produces zero torque. Assert transitions, current bounds,
SOC direction, and real frame delivery. Retain the existing script scenario as
a regression until the real scenario replaces it.

### 5.3 OTA: real request, storage, reboot, rollback

Add a small ota_tool_ecu (or named update-service firmware) rather than growing
the gateway timed OTA script. Use a deliberately compact CAN protocol:
request, fixed-size image chunks, finish/CRC result, status, and abort. This is
a deterministic dogfood transport, not a claim to implement a production
secure bootloader.

Gateway responsibilities:

1. Admit an update only when not DRIVE, not plugged in, no critical BMS fault,
   and required state is fresh.
2. Enter OTA_UPDATE, broadcast it, and rely on the independent powertrain clamp
   to disable torque.
3. Drive the A/B slot model from persistent metadata and real chunks. Trace
   transition, slot, CRC, reset recovery, and abort reason as structured events.
4. On abort, clear pending target atomically and retain the known-good slot.

Use this fault matrix. Keep current rollback fixtures, but only count their
real OTA-tool equivalents as product-grade coverage.

| Case | Real trigger | Required result |
|---|---|---|
| Corrupt image | Tool sends bad CRC/manifest. | Target never commits; slot A stays active. |
| Interrupted write | Fault stops chunk transfer before completion. | Partial target is discarded; no verify/commit. |
| Power cut before commit | Reset/power fault after verify, before atomic commit. | Verified but uncommitted target never boots. |
| Failed health | New target boots and fails health. | Bootloader rolls back to known-good slot. |
| Gateway reset during update | Generic machine reboot at a documented checkpoint. | Recovery reads persistent metadata and aborts safely. |
| BMS critical fault during update | Plant temperature causes actual BMS fault over CAN. | Gateway aborts OTA and keeps known-good slot. |
| OTA while driving | Driver input establishes DRIVE before request. | Request is rejected; no write begins. |
| OTA while charging | EVSE establishes CHARGING before request. | Request is rejected; no write begins. |

If power cut before write is distinct from interrupted write, add it explicitly
instead of silently relabeling a case. Update the count in BLOCKERS.md only
after the exact mapping is verified.

### 5.4 Dashboard and cockpit: real display state

The cockpit test correctly proves control-plane determinism, but its framebuffer
is empty. Make dashboard firmware produce meaningful frames:

1. Bind board configuration to a specific machine. Extend the gRPC API
   compatibly with an optional target; preserve old behavior only for a single
   unambiguous machine.
2. ConfigureBoard creates devices in that world target DeviceBank and
   InspectDevices queries the same world explicitly.
3. Add a deterministic dashboard renderer for boot, drive, charging, OTA, and
   warning state. Render on a state change or fixed refresh tick so hashes and
   dirty rectangles are stable.
4. Make touch a harmless dashboard-local action, such as page selection or
   acknowledgement of a noncritical warning. Do not let touch bypass vehicle
   safety authority.
5. Reconcile mode/warning events, inspected state, dirty rectangles, and hashes
   in the cockpit test. Run two sessions concurrently after P0.

Do not prioritize a std-only harness cockpit wrapper over real display state.
The sim-grpc integration test remains the correct product-surface home.

### 5.5 Telematics: deterministic protocol, then host socket

Build telematics_ecu on existing Ethernet abstractions only after network state
is per machine/session. Use a compact application protocol with monotonic
request IDs and framed status/ack records.

Deliver it in two levels:

1. Deterministic virtual network: periodic status, remote query, and
   acknowledgement through virtual Ethernet. Assert IDs and response counts
   without host timing.
2. Host-connected integration: start a local loopback TCP server on an
   ephemeral port, run a costar server/session, configure small socket buffers,
   and exercise bursts plus fragmented reads/writes. Record a host transcript
   and assert every request has one response, bytes are conserved, and repeated
   readiness wakeups work.

Use loopback only, bounded timeouts, and complete cleanup. Do not add raw host
sockets to microcar C firmware or mask partial writes with a test-only retry.
The TCP bridge and firmware parser must retain incomplete data until one complete
length-prefixed record is available.

## 6. Remaining Debug-Gym Seeds

Reuse dogfood/src/debug_gym_corpus.rs and its paired failing/fixed shape. Every
new seed needs a real gated buggy firmware variant and the same transport path
as its fixed counterpart.

| Seed | Correct behavior | Buggy variant | Localizing signal |
|---|---|---|---|
| BMS stale sensor | BMS/gateway rejects stale snapshot and publishes correct safety result. | Freshness check is skipped/reversed so old safe data is accepted. | Typed snapshot/freshness event: age, sequence, and resulting fault/mode. |
| Dashboard missed warning | Dashboard receives real warning, updates state, and renders it. | Warning is dropped or a lower-priority state overwrites critical warning. | Warning breakpoint plus inspection/framebuffer at same virtual time. |
| Telematics partial write | Parser buffers fragments and acknowledges each complete request once. | Parser treats partial data as complete or drops remainder. | Host transcript and request-ID trace at the fragment boundary. |

Each seed must retain the existing three checks: bug-reproduced, bug-fixed, and
traces-diverge. Divergence must be the documented debugging primitive, not only
a changed final hash. Do not add a seed that directly invokes a private helper.

## 7. Typed Breakpoints And Nightly/Scale (P5)

Implement predicates over structured events and snapshots, not formatted human
trace strings. Define a stable event vocabulary for vehicle mode, DTC creation,
BMS snapshot freshness, OTA transition/abort, and dashboard state. Device
predicates consume session-owned inspection snapshots.

Add predicates only with a consumer scenario:

1. vehicle_state and dtc_created with real diagnostics scenarios.
2. device_state with dashboard cockpit scenario.
3. assertion_failure with a semantic harness assertion event.

Each predicate needs a synthetic unit test, an end-to-end hit test, a no-hit
test, and keyframe/replay equivalence. continue_until and run_to_frame remain
the underlying primitives; do not add another scheduler path.

Compose nightly only after semantic lanes are individually green. It should run
repeated deterministic lanes, real debug-gym corpus, topology/fleet scale, and
host telematics separately. Classify failures as nondeterminism, timeout,
transport loss, semantic assertion, or panic.

## 8. Patch Boundaries And Verification

### Costar ownership

- sim-devices: DeviceBank and context-aware inspection.
- sim-ffi: active context for time, task identity, devices, and firmware
  instance storage.
- sim-world: receiver-owned CAN/link delivery, plant actuator inbox/callback,
  and restartable machines.
- sim-net: Ethernet, bridges, and host poller under world/machine ownership.
- sim-grpc: board, touch, inspection, and run lifecycle bound to session world.

### Microcar ownership

- common: compact protocol payloads and pure charging/OTA state rules.
- ECU firmware: per-instance mutable state plus real BMS, charging, OTA,
  dashboard, and telematics flows.
- plant: real actuator/charge inputs and deterministic fixed-point sensors.
- src/validate.rs: microcar protocol-level validation while costar retains
  generic scenario parsing.
- dogfood: semantic parsers/scenarios after the real path exists; old trace
  lanes remain regressions during migration.

Use small patches with one observable contract each. Preferred sequence:

    context test
    -> CAN routing
    -> diagnostics over bus
    -> reboot factory
    -> OTA recovery
    -> charging FSM/plant
    -> dashboard/cockpit
    -> telematics
    -> remaining seeds
    -> predicates/nightly

Run focused two-world, CAN, and reboot tests before broad regressions. Then:

    cd /home/zmm/projects/microcar
    cargo build --bin microcar
    cargo test --manifest-path state_tests/Cargo.toml
    cargo test --manifest-path dogfood/Cargo.toml
    MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- diagnostics
    MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- charging
    MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- ota
    MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- debug-gym-corpus

    cd /home/zmm/projects/costar
    PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc cargo test -p sim-world -p sim-ffi -p sim-devices -p sim-grpc

For host telematics, run its integration test separately with an explicit
timeout and transcript artifact. Once a track is real and green, update
docs/BLOCKERS.md with scenario names, whether the path is trace-backed or
bus-backed, and exact verification commands.

## Do Not Take These Shortcuts

- Do not force vehicle modes from TOML or mutate gateway state to prove a mode
  guard.
- Do not count a trace-only reset, BMS fault, or OTA transition as a real matrix
  case.
- Do not make a global registry keyed by session ID and call it isolation.
- Do not use C thread-local globals as per-machine firmware state.
- Do not replace semantic assertions with full golden traces for host I/O.
- Do not remove current green lanes until their real replacements have
  repeated-run stability and cover the same safety property.
