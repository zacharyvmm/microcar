# costar + microcar Dogfood: Remaining-Work Agent Guide

Last audited against the code: 2026-07-14

`costar`: `3b828f5` (`main`)

`microcar`: `088ccf8` (`main`)

This file is the execution contract for finishing the dogfood project. It is
written for implementation agents, not as a historical roadmap. An agent must
take one work packet from Section 6, satisfy every acceptance item in that
packet, and report the commands and results it ran.

`docs/BLOCKERS.md`, `UNBLOCKING.md`, and `HANDOFF.md` contain useful history,
but some of their completion claims do not match the current code. When they
conflict with this guide or an executable test, the code and test win.

## 1. Product Goal

`microcar` must prove these `costar` capabilities through real, independently
executing producers and consumers:

- isolated in-process Worlds and isolated gRPC/JSON-RPC sessions;
- receiver-correct CAN, displays, touch, ADC, timers, storage, and Ethernet;
- restartable firmware whose flash/EEPROM/block data persists while guest RAM,
  tasks, queues, C instance state, and other volatile devices reset;
- diagnostics, charging, and OTA driven by protocol frames, not timed trace
  scripts or direct gateway mutation;
- deterministic dashboard rendering and harmless dashboard-local touch input;
- deterministic telematics over virtual Ethernet and a host loopback TCP
  bridge, including fragmented reads and partial writes;
- seven reproducible debug-gym bugs and structured debugging predicates; and
- bounded-memory, repeatable CI at ordinary, concurrent, fleet, and eight-hour
  virtual-time scale.

A lane is not complete because it emits a desired trace label. Its producer,
transport/device, consumer, and observable effect must all execute, and the
test must inspect both semantic state and transport/device evidence.

## 2. Repository and Authority Boundaries

| Concern | Owner |
|---|---|
| execution context, devices, network ownership, CAN routing, restart, control planes, generic predicates | `costar/crates/*` |
| CAN payloads, vehicle modes, ECU state machines, firmware, charging/OTA rules | `microcar/common` and `microcar/firmware` |
| SOC, temperature, current, speed, charge/discharge behavior | `microcar/plant` |
| automotive scenario validation and ECU classification | `microcar/src/validate.rs` and `microcar/src/lib.rs` |
| dogfood scenarios, semantic assertions, summaries, and subprocess orchestration | `microcar/dogfood` |

Never put vehicle modes, BMS fields, charging rules, or OTA policy in `costar`.
Never implement generic session or device ownership in `microcar`. ECUs may
communicate only through simulated devices/buses. The plant may communicate
with firmware only through its World-managed CAN endpoint.

## 3. How an Agent Must Use This Guide

1. Read the current code named by the packet. Do not copy an API from this
   document over an already-correct implementation.
2. Confirm every prerequisite packet is complete. If it is not, stop and take
   the prerequisite instead.
3. Preserve unrelated user changes and existing human golden traces.
4. Implement the smallest complete vertical slice described by the packet.
   Do not land half of a protocol lane behind unconditional fake trace output.
5. Run the packet's focused tests, then its stated regression gate.
6. Update the status table in Section 5 in the same change. Change a row to
   `DONE` only when all acceptance items pass.
7. Report changed files, observable behavior, exact verification commands,
   and any remaining failure. Do not report a stub, fixture, or pure model as
   an end-to-end lane.

Agents may work concurrently only on packets whose “May run with” entry says
so. Do not assign concurrent agents overlapping `sim-ffi/src/simulator.rs`,
`sim-world/src/world.rs`, `gateway_ecu/src/main.c`, or a single dogfood lane.

Use this assignment text verbatim when handing off a packet:

```text
Implement packet R<N> from docs/costar_microcar_dogfood_plan.md and no later
packet. Verify its prerequisites against the current code, preserve legacy
fixtures/goldens, meet every acceptance item, update the Section 5 status row,
and report changed files plus exact command results. Do not mark the packet
complete if an acceptance item is skipped or replaced by trace-only evidence.
```

## 4. Definition of Done for Every Packet

Every packet must meet all applicable rules:

- Default behavior remains backward compatible; existing golden traces are
  byte-identical unless the packet explicitly replaces a golden and explains
  why.
- New behavior is enabled by a new scenario/firmware selection or a compatible
  optional API field.
- Tests assert typed state and real transport/device evidence. A local
  `sim_trace_*` record may aid diagnosis but cannot be the only assertion.
- Deterministic tests use virtual time and stable ordering, never host timing.
- Host-connected tests use loopback, bounded wall-time timeouts, byte/request
  conservation, and cleanup assertions rather than golden traces.
- No production path falls back to “the last active machine” or a process-wide
  device/network store.
- The implementation has focused unit tests, an end-to-end test, and a
  repeated-run determinism test where the packet requests one.
- Documentation describes implemented behavior in present tense only after the
  behavior is green.

Keep the legacy trace-script diagnostics, charging, and OTA fixtures as
regressions until their protocol-backed replacements pass 100 repetitions.

## 5. Audited Starting State

Status meanings:

- `DONE`: implemented and covered by the current code.
- `CORE ONLY`: reusable primitive exists; product integration is absent.
- `SCAFFOLD`: files exist but are stubs, synthetic fixtures, or currently fail.
- `TODO`: implementation is absent.

| Area | Status | What is actually true |
|---|---|---|
| DeviceBank, explicit machine targeting, owned CAN, restart algorithm | DONE | `with_machine_devices`, owned banks, persistent-device snapshot/restore, boot boundary, gRPC machine IDs, and core acceptance tests exist. |
| Unified run control and gRPC session resource bounds | DONE | `drive_world`, per-session gRPC locks, deterministic listing, TTL, trace ring, and keyframe limits exist. |
| JSON-RPC per-session locking | SCAFFOLD | `sim-runner/src/serve.rs` still stores `BTreeMap<u64, Session>` under one global mutex. |
| Per-machine time and task identity | DONE | `GuestRuntime` accessors (`active_now`, `active_task_id`, etc.) wired into all C ABI paths; global atomics kept as legacy fallback only. |
| C firmware instance isolation | DONE | All eight ECUs use `sim_instance_state` with unique keys; zero mutable file/function statics remain. |
| NetworkBank | DONE | `Simulator` and `SimulatorExecutionContext` own an optional `NetworkBank`; `enable_owned_network()` activation scoped alongside `SimGlobal`, `DeviceBank`, and `GuestRuntime`. |
| Protocol IDs/payload structs and external-actor validation | DONE | Stage C IDs and packed structs exist; validation has tests. |
| ECU role classification | DONE | `gateway_diag*` variants correctly classify as `gateway`; table-driven tests cover every firmware variant. |
| Live diagnostics | SCAFFOLD | BMS emits sequenced status but dispatches the wrong input ID; gateway returns `UNSUPPORTED` for live BMS; tool does not issue/assemble selectors. |
| Charging | CORE ONLY | C FSM and battery signed-current method exist. Rust FSM mirror is a one-test stub; firmware and plant CAN loop are not wired. |
| OTA persistence model | CORE ONLY | The 32-byte metadata model and Rust mirror exist. Gateway still runs an in-RAM timed script, and OTA tool uses incompatible local message definitions. |
| Dashboard | CORE ONLY | C renderer and FreeRTOS display task exist. The authoritative gRPC test uses synthetic Rust firmware, not microcar firmware; Zephyr and product-session coverage are incomplete. |
| Telematics | SCAFFOLD | ECU sends a custom periodic record. C parser and Rust mirror are TODO stubs; current harness is a trace/determinism smoke test only. |
| Typed predicates | SCAFFOLD | Types and semantic storage exist, but `Device` always returns `false`; firmware events are not connected to the World; end-to-end/replay tests are absent. |
| TraceStats and soak | CORE ONLY | A line-based accumulator exists in `sim-core`; it is not wired to World/CLI, and no Rust soak harness exists. |
| Debug corpus | SCAFFOLD | Four seeds are registered and all pass (R0 fixed classification). Three additional directories exist but are not real or registered. |

Known command results at this audit point:

```text
harness diagnostics       0/4 pass
harness charging          1/6 pass (only legacy plug_blocks_drive)
harness ota               5/13 pass (the five legacy script fixtures)
harness debug-gym-corpus  1/4 pass (classification breaks three)
harness telematics        pass, but only as a trace smoke/determinism check
```

Many files under `dogfood/diagnostics`, `dogfood/charging`, `dogfood/ota`, and
`dogfood/debug_gym` are specifications-in-TOML, not completed scenarios. In
particular, `sender = "plant"` is invalid because there is no passive machine
named `plant`; final scenarios must consume sensor frames published by the real
`[plant]`, not repair this by inventing a fake plant sender.

## 6. Work Packet Order

The required order is:

```text
R0
 └─ R1 ─ R2 ─ R3 ─ R4 ─ R5
                       ├─ R6 diagnostics
                       ├─ R7 charging
                       └─ R8 telematics
R6 + R7 ──────────────── R9 OTA
R5 + R6/R7/R9 as inputs ─ R10 cockpit
R6 + R8 + R10 ────────── R11 debug corpus + predicates
R6..R11 ──────────────── R12 soak + CI + final documentation
```

R6 and R7 may run concurrently after R5 if agents coordinate ownership of
`gateway_ecu/src/main.c`; otherwise run them sequentially. R8 may run in
parallel with R6/R7 because it owns the network and telematics surfaces. R9
must follow both R6 and R7 because OTA admission depends on fresh BMS state and
charging state. R10 should follow the real vehicle states it displays.

### R0 — Restore trustworthy ECU classification

**Goal:** legacy gateway variants are recognized as gateway authorities, so
baseline failures are product failures rather than validator mistakes.

**Files:**

- `microcar/src/lib.rs`
- `microcar/src/validate.rs`

**Implementation:**

1. Keep boot dispatch variants distinct, but map these paths to category
   `gateway`, not `diagnostics`: `gateway_diag`, `gateway_diag_fault`,
   `gateway_diag_clear`, `gateway_diag_clearbug`,
   `gateway_diag_startdrive`, and `gateway_diag_startdrivebug`.
2. Keep `diagnostics_tool_ecu` categorized as `diagnostics`.
3. Add table-driven tests for every firmware variant in
   `MicrocarFirmware::ecu_type`/`resolve_ecu_category`; assert boot variant and
   semantic category separately so this cannot regress again.
4. Do not weaken the missing-gateway validator.

**Acceptance:**

- `cargo test -p microcar` passes.
- `harness debug-gym-corpus` returns 4/4 without changing the four scenarios.
- The valid legacy diagnostics scenarios no longer fail `missing-gateway`.
- The staged live diagnostics files may remain red until R6; do not hide them
  by deleting or silently skipping them.

**May run with:** nothing; do this first.

### R1 — Move virtual time and task identity into GuestRuntime

**Goal:** two simulators on one host thread cannot observe each other's virtual
clock or current task ID.

**Files:**

- `costar/crates/sim-ffi/src/guest_runtime.rs`
- `costar/crates/sim-ffi/src/lib.rs`
- `costar/crates/sim-ffi/src/device_ffi.rs`
- `costar/crates/sim-ffi/src/net_ffi.rs`
- `costar/crates/sim-ffi/src/zephyr_ffi.rs`
- `costar/crates/sim-ffi/src/simulator.rs`
- `costar/crates/sim-ffi/include/sim_abi.h`

**Implementation:**

1. Add internal accessors `active_now`, `set_active_now`,
   `active_task_id`, and `set_active_task_id`. When a `GuestRuntime` is active,
   they read/write its `Cell`s. Only when no runtime is active may they use the
   existing atomics for legacy unit tests.
2. Replace every production read/write of `SIM_NOW` and `CURRENT_TASK_ID` with
   those accessors, including FreeRTOS, Zephyr, device, network, trace, timeout,
   ISR, and callback paths. Direct atomic use may remain only inside the
   fallback accessors and explicit fallback tests.
3. Set active time before every guest callback/fiber resume. Set the active
   task ID immediately before a task resumes and clear it immediately after it
   yields/exits, including panic unwind.
4. Keep `sim_instance_state` allocation as implemented. Restart gets a new
   `GuestRuntime`; it must not copy `now`, task ID, or instance regions from the
   old runtime.

**Acceptance:**

- A two-simulator interleave test assigns different times/task IDs and proves
  each FFI callback observes only its owner in A/B and B/A order, 100 times.
- Nested activation and panic unwind restore the prior runtime.
- FreeRTOS and Zephyr task-ID traces remain correct.
- Existing costar and microcar golden traces remain byte-identical.

**Focused gate:**

```bash
cd /home/zmm/projects/costar
cargo test -p sim-ffi -p sim-freertos-port -p sim-zephyr-port
```

**May run with:** none; R1 owns shared execution-context code.

### R2 — Finish per-instance C firmware state

**Goal:** duplicate instances of an ECU type and rebooted firmware have
independent mutable C state.

**Files:**

- `microcar/firmware/gateway_ecu/src/main.c`
- `microcar/firmware/powertrain_ecu/src/main.c`
- `microcar/firmware/diagnostics_tool_ecu/src/main.c`
- audit the other five ECU `main.c` files and `microcar_coordinator.c`

**Fixed keys:**

| Key | ECU |
|---:|---|
| `0x4D430001` | gateway |
| `0x4D430002` | powertrain |
| `0x4D430003` | BMS |
| `0x4D430004` | dashboard FreeRTOS |
| `0x4D430005` | dashboard Zephyr |
| `0x4D430006` | diagnostics tool |
| `0x4D430007` | OTA tool |
| `0x4D430008` | telematics |

**Implementation:**

1. Create one context struct per ECU and obtain it through
   `sim_instance_state(key, sizeof(ctx), alignof(ctx))`.
2. Move every mutable file/function static and every campaign cursor that
   survives a loop iteration into the context. For gateway this includes
   `g_gs`, `g_hm`, `g_fm`, diagnostic/charging/OTA flags, OTA slot state,
   mutexes, event groups, queues, semaphores, and task handles. For powertrain
   it includes controller/watchdog state, bug flags, semaphores, timer, and task
   handle. Remove the function-static OTA state and the comment claiming one
   gateway per process.
3. Give diagnostics key `0x4D430006` and store request/script cursors and any
   decoded snapshot there. Task-stack locals that do not survive a yield need
   not be moved.
4. Task entry/callback functions must obtain or receive their own ECU context;
   they must never retain another machine's pointer.
5. Check `sim_instance_state` for null and fail the firmware boot with a
   traceable error. Immutable `static const` tables may remain global.
6. Do not `memset` an already-running context on every accessor call. Initialize
   exactly once in the firmware boot path. A reboot naturally receives a new
   zeroed region.

**Acceptance:**

- Two gateway instances in one World have different modes, DTCs, queues, and
  task IDs after different inputs.
- Two powertrain instances given different modes/driver requests publish
  different torque without cross-observation.
- Reboot resets only the selected ECU context and reruns initialization;
  sibling context is unchanged.
- Audit all eight ECU `main.c` files for mutable file-scope/function-scope
  static variables (excluding `static const` tables and `static` functions).
  All mutable storage must reside in the per-instance context struct obtained
  via `sim_instance_state`. Immutable `static const` lookup tables and
  `static` function declarations are allowed.
- Existing legacy dogfood fixtures retain their hashes.

**May run with:** none; complete after R1 and before R3.

### R3 — Activate NetworkBank per machine

**Goal:** network device 0, TCP framing, and host poller state are isolated and
destroyed with their owning machine/session.

**Files:**

- `costar/crates/sim-net/src/bank.rs` and network accessors
- `costar/crates/sim-ffi/Cargo.toml`
- `costar/crates/sim-ffi/src/simulator.rs`
- `costar/crates/sim-ffi/src/net_ffi.rs`
- `costar/crates/sim-world/src/machine.rs`
- session reset/clone/rebuild paths in both servers

**Implementation:**

1. Add an owned `NetworkBank` handle to `Simulator` and
   `SimulatorExecutionContext`, parallel to `DeviceBank`. Activating a machine
   must activate `SimGlobal`, `DeviceBank`, `GuestRuntime`, and `NetworkBank` in
   one unwind-safe lexical scope.
2. Production Worlds and both server session loaders must enable owned network
   banks before firmware attachment. Keep fallback registries only for direct
   legacy single-simulator tests.
3. Route Ethernet devices, smoltcp bridges, TCP framing buffers, TAP/TCP
   handles, host poller registrations, readiness state, and cleanup through the
   active bank.
4. World packet injection and TX drain must explicitly activate the sender or
   receiver machine. Never select a network bank from the most recently active
   context.
5. Restart creates an empty network bank. Session destruction must close host
   handles and remove every poller registration; clone/reset/keyframe rebuild
   must not retain the old host readiness state.

**Acceptance:**

- Two Worlds with Ethernet device 0 conserve distinct frames in both
  interleave orders for 100 repetitions.
- Two fragmented TCP streams using the same IDs retain only their own partial
  bytes.
- Destroy/recreate of a session yields no stale readiness event, handle, or
  buffered byte.
- A panic inside one active network context restores the sibling context.

**Focused gate:**

```bash
cd /home/zmm/projects/costar
cargo test -p sim-net -p sim-ffi -p sim-world
```

**May run with:** none; R3 overlaps the execution context established by R1.

### R4 — Finish control-plane and restart residuals

**Goal:** both control planes have the same lock/lifecycle behavior, and the
microcar reboot fixture proves firmware recovery rather than reset markers.

**Files:**

- `costar/crates/sim-runner/src/serve.rs`
- `costar/crates/sim-grpc/src/session.rs`
- `costar/crates/sim-grpc/src/server.rs`
- `costar/crates/sim-grpc/tests/*`
- `microcar/dogfood/b3_gateway_reboot_downtime.toml`
- a microcar restart integration test/harness assertion

**Implementation:**

1. Change JSON-RPC storage to
   `Mutex<BTreeMap<u64, Arc<Mutex<Session>>>>`. Hold the map lock only for
   lookup/insert/remove; lock one session for operations. Run uses the existing
   take/return-World pattern and `drive_world`.
2. Every success, stop, disconnect, error, and panic path must return the World
   or mark the session `Error`. Natural completion/Stop => `Done`; disconnect =>
   `Paused`; simulation error/panic => `Error`.
3. Add the missing gRPC `failed_session_returns_world_and_sibling_runs` test.
   Register a test firmware factory that deliberately errors/panics; do not add
   a product-only panic hook.
4. Retain the fixed limits already implemented: 128 sessions, 16 keyframes,
   100,000 trace records with drop counter, 300-second eligible-state TTL, and
   cleanup no more often than every 30 host seconds plus create/list.
5. Extend the gateway downtime fixture/integration assertion to prove: second
   real firmware boot, heartbeat resumes after downtime, RTOS/config is
   preserved, flash survives, volatile queues/C state reset, and a sibling
   heartbeat is uninterrupted. Reset markers alone do not pass.

**Acceptance:**

- JSON-RPC and gRPC outcomes match for completion, deadline, stop, disconnect,
  error, and panic.
- Deterministic listing, limits, eviction, and TTL tests pass for both servers.
- The failure/panic test leaves a sibling runnable and the failed session
  inspectable in `Error`.
- The microcar reboot test passes 100 repetitions.

**May run with:** none; finish the control-plane gate before R5.

### R5 — Full duplicate-world/session isolation gate

**Goal:** close the infrastructure gate before product lanes claim real
concurrent isolation.

**Implementation and acceptance tests:**

- Two real Worlds with duplicate gateway and BMS firmware reproduce their solo
  Trace v2 hashes under A/B and B/A interleaving, 100 times.
- Two BMS instances receive different real plant frames and publish different
  status/limits without cross-observation.
- Two concurrent gRPC sessions load real microcar firmware, configure the same
  display/touch/timer/ADC/CAN/Ethernet device IDs, inject different inputs, and
  reproduce their solo hashes, 100 times.
- Reboot resets selected C/device/network volatile state while preserving only
  selected persistent devices and every sibling state.
- Frames arriving before `boot_at` are absent; a frame at `boot_at` is received
  once after boot.

Place cross-repository product tests in `microcar/tests` with root-package
dev-dependencies on `sim-grpc`, `tokio`, and `tonic`. Keep the
`microcar-dogfood` library std-only. Generic costar tests must use generic test
firmware and must not depend on microcar.

No later packet may call a lane session-isolated until R5 is green.

**May run with:** none.

### R6 — Real diagnostics over CAN

**Prerequisite:** R5.

**Goal:** values travel plant -> BMS -> gateway -> diagnostics tool over CAN,
with freshness and correlation proved.

**Files:**

- `microcar/firmware/bms_ecu/src/main.c`
- `microcar/firmware/gateway_ecu/src/main.c`
- `microcar/firmware/diagnostics_tool_ecu/src/main.c`
- `microcar/dogfood/src/diagnostics.rs`
- `microcar/dogfood/diagnostics/*.toml`

**Wire behavior:**

1. BMS consumes `MC_MSG_PLANT_SENSORS` (`0x500`), not
   `MC_MSG_BMS_STATUS`. After every valid 7-byte plant update it increments an
   8-bit wrapping sequence and publishes the existing 8-byte
   `MC_MSG_BMS_STATUS` (`0x200`).
2. Gateway caches the complete status, sequence, and receive time. It is fresh
   exactly when `age_ms <= 500`.
3. For `MC_DIAG_LIVE_BMS`, use request `param` selectors:
   - `0`: `value0=soc_percent`, `value1=temp_c + 40`;
   - `1`: pack voltage in 100 mV as little-endian `u16`;
   - `2`: pack current in 100 mA as little-endian `i16`.
4. If no snapshot exists or `age_ms > 500`, return `MC_DIAG_STALE` and two zero
   values. Unsupported selectors return `MC_DIAG_UNSUPPORTED`.
5. Diagnostics tool sends selectors 0, 1, and 2 with distinct nonzero request
   IDs and assembles one snapshot only after three successful matching
   responses. Mismatched/duplicate IDs do not complete it.

**Scenarios:**

- Repair `live_bms_data.toml` to use real `[plant]` publications; remove every
  fake `sender = "plant"` injection.
- Repair `stale_bms_data.toml`: receive a valid snapshot, stop the BMS machine,
  wait 501 ms, query, and require `STALE`.
- Add `actuator_test_rejected_in_drive.toml`; establish DRIVE by normal driver
  input and gateway authority, then issue the tool request.
- Preserve the two legacy script scenarios as regressions.

**Harness acceptance:**

- Assert decoded values equal the plant values with the scaling above.
- For each response, find Trace v2 edges tool TX -> gateway RX and gateway TX
  -> tool RX. Each leg shares its own correlation ID; the protocol request ID
  joins request and response.
- Assert sequence and age, and assert stale values are zero.
- All five diagnostics scenarios pass, then the three real cases pass 100
  repetitions with stable normalized Trace v2 hashes.

Do not parse formatted human trace lines for the authoritative checks. Run the
binary with `--trace-v2` and parse structured JSONL.

**May run with:** R8. It may run with R7 only if one agent at a time owns and
lands changes to `gateway_ecu/src/main.c`.

### R7 — Closed-loop charging

**Prerequisite:** R5. Coordinate gateway ownership with R6.

**Goal:** EVSE and BMS frames drive a gateway-owned FSM; commands reach the
plant through CAN; SOC/current/temperature and torque safety change for real.

**Step 1: finish the pure model.**

- Replace `state_tests/src/charging.rs` with a complete Rust mirror and tests.
- Fix C transition precedence: in any plugged state, `critical_fault` wins
  first, then `UNPLUG`; only then evaluate state-specific transitions.
- An accepted transition must update `*cs` and return the same state in
  `output.next_state`. A rejected event leaves `*cs` unchanged, commands zero,
  and returns nonzero `reject_reason`.
- Implement only:

```text
DISCONNECTED + PLUG                 -> PLUG_DETECTED
PLUG_DETECTED + HANDSHAKE_OK        -> HANDSHAKE
HANDSHAKE + fresh BMS limit > 0      -> ACTIVE
ACTIVE + lower nonzero BMS limit     -> LIMITED
ACTIVE/LIMITED + soc >= target_soc   -> COMPLETE
any plugged state + critical fault   -> FAULT
any plugged state + UNPLUG           -> DISCONNECTED
```

Vehicle mode is `CHARGING` from `PLUG_DETECTED` through `COMPLETE`; charging
`FAULT` maps to vehicle `FAULT`. Command current is
`min(EVSE offered, BMS limit)` in 0.5 A units. Clamp target SOC to 50..100.
BMS limits are 64 below 45.0 C, 32 from 45.0 through 59.9 C, and 0 at 60.0 C
or during a critical fault.

**Step 2: add the plant CAN endpoint.**

1. Add a default no-op `EnvironmentModel::receive_can(...)` taking arrival
   time, bus, sender, message ID, and payload.
2. World maintains a deterministic plant inbox populated after bus latency,
   corruption/drop, and ordering are resolved. De-duplicate receiver copies by
   logical bus sequence so the plant receives one command per transmission.
   Drain this inbox before the plant's next `step`.
3. `MicrocarPlant` consumes `MOTOR_COMMAND` and `CHARGE_COMMAND`. After its
   first real motor command, never map throttle directly to torque again.
4. While charging, call
   `step_with_current(-(current_a_x2 as i32) * 500, dt_ms)`; otherwise retain
   the existing deterministic discharge model. Positive current discharges,
   negative current charges. Saturate sensor current to `i16` on CAN.

**Step 3: wire firmware.**

- BMS publishes `BMS_CHARGE_LIMIT` from real plant sensor state and increments
  its sequence.
- Gateway consumes `EVSE_EVENT` and fresh BMS limits, owns the charging state,
  broadcasts vehicle mode, and sends `CHARGE_COMMAND` after every accepted
  transition/limit change. Invalid events generate a nonzero reason without a
  state change.
- Powertrain independently clamps torque whenever its received mode is not
  DRIVE or LIMP. It must not trust a gateway-local helper.

**Scenarios and acceptance:**

Repair the five scaffold files `plug_handshake_active`,
`high_temperature_limited`, `charge_complete`, `bms_fault_stops_charging`, and
`drive_while_plugged`. Remove fake plant/BMS state injections where real
firmware must produce them. Keep `plug_blocks_drive` as a legacy regression.

The harness must assert the exact FSM sequence, CAN TX/RX edges, commanded
current never exceeding either limit, SOC increase, deterministic temperature,
zero current on fault, and zero torque while plugged. All six scenarios pass;
the five real scenarios pass 100 repeats with stable hashes/final state.

**May run with:** R8. It may run with R6 only under the gateway ownership rule
above.

### R8 — Telematics and host networking

**Prerequisite:** R5/R3.

**Goal:** complete records survive every fragment boundary and burst, with one
response per request and no cross-session or stale host state.

**Files:**

- `microcar/common/include/microcar_telematics.h`
- `microcar/common/src/microcar_telematics.c`
- `microcar/state_tests/src/telematics.rs`
- `microcar/firmware/telematics_ecu/src/main.c`
- `microcar/dogfood/src/telematics.rs`
- `microcar/dogfood/telematics/*.toml`
- host bridge integration tests in `costar/crates/sim-net/tests` or
  `microcar/tests`

**Protocol:** records are big-endian length-prefixed:

```text
u16 payload_length | u8 type | u32 request_id | payload
```

`payload_length` counts `type + request_id + payload`, so it is at least 5 and
at most 256. Types: `1=STATUS`, `2=DIAG_QUERY`, `3=LOCK`, `4=UNLOCK`,
`5=PRECONDITION`, `6=FAULT_UPLOAD`, `0x80=ACK`, `0x81=ERROR`.

Payloads after type/request ID:

- STATUS: `[vehicle_mode, soc_percent, temp_c_x10_le16, fault_code]`;
- DIAG_QUERY: `[selector]` using R6 selectors;
- LOCK/UNLOCK: empty;
- PRECONDITION: `[target_temp_c_i8]`;
- FAULT_UPLOAD: `[source_node, fault_code, severity]`;
- ACK: `[original_type, status=0, response_data...]`;
- ERROR: `[original_type, error_code]`.

Request IDs begin at 1 and increase monotonically. Each request receives
exactly one ACK or ERROR with the same ID. Length below 5 or above 256 => error
1; unknown type => error 2; invalid payload => error 3.

**Implementation:**

1. Delete the current incompatible custom periodic format and duplicate local
   `MC_NODE_TELEMATICS` definition; use common protocol constants (node 8).
2. Implement a bounded incremental C parser. It retains incomplete 2-byte
   headers and payloads, consumes multiple records per read, never reads past
   258 stored bytes, and returns explicit complete/error results.
3. Implement the same state machine in Rust and exhaustive tests: every byte
   split, header split, payload split, coalesced records, 100-record burst,
   invalid lengths/types/payloads, duplicate IDs, and buffer reset.
4. Firmware caches real CAN vehicle/BMS/fault state, publishes STATUS every
   1,000 ms, processes every remote command, and handles partial network sends
   by retaining the unsent suffix.

**Integration acceptance:**

- Virtual Ethernet test covers periodic status and every request type with
  deterministic IDs/payloads and one response per request.
- Host loopback TCP uses an ephemeral port and 256-byte socket buffers. Split
  every request at every byte boundary, then send a 100-record burst. Enforce a
  10-second wall timeout and assert byte conservation, repeated poller wakeups,
  response uniqueness, and total socket/poller cleanup after destruction.
- Run two concurrent network-device-0 sessions and compare each to its solo
  transcript.
- Replace the trace-smoke `harness telematics` with semantic JSON output for
  both levels. Host tests remain separate from golden traces.

**May run with:** R6 and R7; R8 does not modify gateway firmware.

### R9 — OTA through CAN, flash, and restart

**Prerequisites:** R6 and R7.

**Goal:** OTA bytes travel tool -> CAN -> gateway -> virtual flash -> reboot
recovery, with admission and rollback proved from persistent metadata.

**Persistent layout on gateway flash device 0 (256-byte pages):**

| Pages | Purpose |
|---|---|
| 0 | metadata A |
| 1 | metadata B |
| 2-31 | slot A image |
| 32-61 | slot B image |
| 62-63 | reserved |

Use the existing exact 32-byte little-endian record:

```text
0..3   magic 0x4D434F54
4      format_version = 1
5      active_slot
6      target_slot
7      state
8      committed
9      boot_attempt_count
10     healthy
11     abort_reason
12..15 generation u32
16..19 image_length u32
20..23 image_crc32 u32
24..27 reserved = 0
28..31 CRC32 of bytes 0..27
```

For each metadata update: select the valid highest generation, erase/write the
copy with lower generation using `generation + 1`, read it back and validate
CRC, then erase the old page. On boot choose the valid highest generation; if
neither is valid, initialize slot A known-good. CRC32 is reflected IEEE
polynomial `0xEDB88320`, init/final XOR `0xFFFFFFFF`.

**Implementation:**

1. Keep the pure metadata model and Rust mirror. Add any missing tests for
   torn writes, equal generations, invalid CRC, abort, and reset recovery.
2. Replace OTA tool's local `0x630..0x633` definitions with common
   `OTA_REQUEST`, `OTA_CHUNK`, `OTA_FINISH`, and `OTA_STATUS`. Test chunks carry
   five data bytes.
3. Gateway admission requires: not DRIVE, charging DISCONNECTED, no critical
   BMS fault, BMS age <= 500 ms, and no active update. Rejection sends status
   and performs no erase/write.
4. After admission: enter OTA_UPDATE and broadcast it; erase inactive slot;
   require in-order chunks; write each chunk to flash; verify count and CRC;
   write commit metadata; request generic reboot with 10 ms downtime.
5. Boot recovery reads flash, never surviving RAM. A committed target gets one
   health attempt. Success marks target active/healthy. Failure writes rollback
   metadata and reboots the known-good slot. Powertrain clamps torque throughout.
6. Delete the function-static/timed gateway OTA implementation from the real
   firmware path. Keep named legacy script variants only as regression firmware.

**Scenarios:** first rename the current trace-script `happy_path.toml` to
`legacy_happy_path.toml`; then create a real tool-driven `happy_path.toml`.
Repair the other eight scaffold files `corrupt_image`,
`interrupted_write`, `power_cut_before_commit`, `failed_health`,
`gateway_reset_during_update`, `bms_fault_during_update`,
`ota_rejected_drive`, and `ota_rejected_charging`. Passive actors may inject
tool/EVSE frames; BMS and gateway state must be produced by real firmware.
Keep the four `rollback_*` trace fixtures plus the old scripted happy path as
legacy regressions.

**Acceptance:**

- `harness ota` distinguishes and passes 9 real + 5 legacy cases.
- Trace v2 proves request/status CAN causality and powertrain clamp.
- Add `microcar/tests/ota_flash_integration.rs` (or equivalent in-process
  product test) that directly reads flash device 0 before reset, after boot,
  and after rollback. It asserts bytes, record CRC/generation, active slot, and
  volatile worker reset. A firmware-local trace is not flash inspection.
- Each real case passes 100 repetitions with identical final metadata and
  normalized trace hash.

**May run with:** none; R9 owns gateway admission/update state.

### R10 — Real dashboard and concurrent cockpit

**Prerequisite:** R5 and the real states it displays.

**Goal:** real FreeRTOS/Zephyr dashboard firmware drives virtual display/touch;
gRPC streams deterministic isolated frames.

**Rendering contract:** 320x240 RGB565 little-endian. Clear the full frame on a
screen-state change; render on state change or every 100 ms. Borders are white,
one pixel. Bars fill left-to-right using integer floor. Speed/torque use fixed
seven-segment digits with 8-pixel strokes. No fonts.

| State | Background | Required regions |
|---|---:|---|
| boot | `0x0000` | white `(40,108,240,24)` |
| READY | `0x0010` | green `(0,0,320,40)`, speed `(20,70,120,80)` |
| DRIVE | `0x0200` | green status, speed, torque `(180,70,120,80)` |
| LIMP | `0xFD20` | amber status, speed, torque |
| FAULT | `0x7800` | red warning `(0,170,320,70)` |
| CHARGING | `0x4010` | SOC `(20,90,280,24)`, current `(20,140,280,24)` |
| OTA_UPDATE | `0x0008` | progress `(20,110,280,24)` |

OTA progress: IDLE 0, DOWNLOADING 25, VERIFYING 50, COMMIT_PENDING 75,
REBOOTING 90, HEALTHY 100, ROLLED_BACK 0 percent.

**Implementation:**

1. Use `common/microcar_dashboard.c` from both dashboard variants. Finish the
   Zephyr display/touch task and remove duplicated rendering logic.
2. Mark only rectangles whose pixels changed dirty. Do not redraw the full
   frame at every 100 ms when content is unchanged.
3. A touch **release** inside `(280..319, 0..39)` toggles page 0/1 exactly once;
   press/move do nothing. Touch never changes gateway mode, DTCs, warnings,
   charging, or OTA.
4. Preserve critical warning priority: a later ordinary mode repaint cannot
   overwrite a displayed critical warning until the warning is cleared.
5. Keep the generic synthetic `sim-grpc` cockpit test as a costar regression.
   Add the authoritative product gRPC test in `microcar/tests/cockpit_grpc.rs`
   using `FirmwareRegistry` factories for real microcar firmware. Do not make
   costar depend on microcar and do not add dependencies to the std-only
   dogfood library.

**Product test acceptance:**

- Two sessions run concurrently in one server with different mode sequences,
  both using dashboard machine ID 4 and display/touch ID 0.
- Frames are nonempty; `DisplayFrame.machine_id == 4`; full row-major byte
  hashes for all seven screens match checked-in FNV-1a constants generated only
  after the renderer is fixed.
- Dirty rectangles and hashes repeat exactly; inspected device/touch state
  agrees with the frame at the same virtual time.
- Touch changes only page; sessions never cross-observe pixels/events.
- FreeRTOS and Zephyr hashes match for identical inputs.
- `harness cockpit` invokes the authoritative product test, not only a name
  filter that happens to run the synthetic costar test.

**May run with:** none; run after the real displayed states are stable.

### R11 — Remaining debug corpus and typed predicates

**Prerequisites:** R6, R8, R10; OTA semantic events from R9.

#### R11a. Three remaining debug seeds

Register and implement the existing paired directories:

- `bms_stale_sensor_bug`: buggy gateway skips/reverses the 500 ms freshness
  check; fixed firmware returns STALE. Localizer: snapshot age + sequence.
- `dashboard_missed_warning_bug`: buggy renderer allows normal mode repaint to
  overwrite a critical warning. Localizer: warning event + framebuffer hash.
- `telematics_partial_write_bug`: buggy parser discards incomplete record
  suffix. Localizer: request ID + fragment boundary + host transcript.

Each pair uses the same real path as its product lane and must pass
`bug-reproduced`, `bug-fixed`, and `traces-diverge`. Do not invoke private
helpers or replace the subsystem with trace labels. The corpus passes 7/7.

#### R11b. Connect typed predicates end to end

The existing types are scaffolding, not complete. Implement this generic C ABI
in `sim-ffi`:

```c
typedef enum { SIM_SCALAR_BOOL, SIM_SCALAR_U64, SIM_SCALAR_I64,
               SIM_SCALAR_STRING } sim_scalar_kind_t;
typedef struct {
    const char *name;
    sim_scalar_kind_t kind;
    union { uint8_t b; uint64_t u64; int64_t i64; const char *str; } value;
} sim_semantic_field_t;
void sim_semantic_event(const char *event_type,
                        const sim_semantic_field_t *fields,
                        uint32_t field_count);
```

Copy strings/fields immediately into the active machine's pending-event queue;
reject null/invalid UTF-8 and more than 16 fields with a traceable error. World
drains pending events after each firmware step and adds the active machine ID.
Do not parse formatted human trace lines into semantic events.

Microcar emits exactly:

- `vehicle_state` (`mode`, `fault_code`);
- `dtc_created` (`source`, `code`, `severity`);
- `bms_snapshot` (`seq`, `age_ms`, `fresh`, `soc`, `temp_c_x10`);
- `ota_transition` (`state`, `active_slot`, `target_slot`, `reason`);
- `dashboard_state` (`mode`, `warning`, `page`, `framebuffer_hash`).

Complete `ContinuePredicate::Device` using explicit machine device context for
all existing conditions: display enabled, backlight, touch pending, CAN RX
length, and CAN TX length. Missing machine/device is a non-match, not fallback.

`DroppedFrame` must match an actual observed drop, not merely a configured drop
policy. Record actual `(bus, message_id)` drops in World. Wire scenario/harness
assertion failures into `record_assertion_failure`.

For each of the four predicate variants, and for every DeviceCondition, add:
unit hit, unit miss, real end-to-end hit, and keyframe/replay-equivalence tests.
Reuse `continue_until_predicate`; do not add a scheduler loop.

**May run with:** none; the three seeds and predicates share product evidence.

### R12 — Bounded Rust soak, CI, and closeout

**Prerequisite:** all earlier packets.

**Goal:** 10-minute, 1-hour, and 8-hour scenarios run through the Rust
simulator with bounded trace/output memory and repeatable final state.

**R12a. Correct and wire TraceStats.**

The current `TraceStats` is only a prototype. Wire it before trace retention and
eviction in World. Default remains unbounded. Soak retention is 4,096 records.
Time monotonicity is per `(machine_id, source)` Trace v2 stream, not just the
machine prefix of formatted human lines. Counts and normalized hash cover the
entire pre-eviction stream. Track:

```rust
event_count, normalized_fnv1a64,
first_virtual_time, last_virtual_time, time_regressions,
can_tx_by_id, can_rx_by_id, dropped_by_id,
assertion_failures, retained_records, evicted_records
```

Requested frame drops are allowed only when the scenario declares them; all
other conservation failures fail the lane.

Add CLI flags:

```text
--soak-summary-json <path-or->
--trace-retention <record-count>
```

Summary mode suppresses human trace output and writes exactly one JSON object.
It includes scenario, virtual ticks, wall milliseconds, terminal status, every
TraceStats field, final mode/SOC/temperature/speed, and assertion results. Exit
0 only when simulation and assertions pass.

**R12b. Add `harness soak`.**

```text
harness soak [--level long|hour|overnight|all] [--repeats N]
             [--timeout-secs N] [--json OUT]
```

Mapping/defaults:

- `long`: `scenarios/long_drive_10min.toml`, timeout 120 s;
- `hour`: `scenarios/soak_1hour.toml`, timeout 300 s;
- `overnight`: `scenarios/overnight_8hour.toml`, timeout 900 s;
- `all`: all three in that order;
- default level `long`, default repeats 3.

Invoke the Rust binary with `--trace-retention 4096 --soak-summary-json -`.
Parse the single JSON object while retaining only 20-line stdout/stderr tails;
never call `read_all_lines` or populate `ScenarioRun.trace` in soak mode.

On Linux sample `/proc/<pid>/status` `VmRSS` every 100 ms. Fail on timeout,
panic, nonzero exit, assertion failure, time regression, unexpected CAN loss or
duplication, differing repeat hash/final state, retained records above 4096,
peak RSS above 256 MiB, or eight-hour peak more than 32 MiB above one-hour
peak. Non-Linux omits only RSS assertions and reports them unsupported.

Add unit/integration tests for eviction, pre-eviction hashing, per-stream
monotonicity, requested-drop accounting, summary parsing, timeout/panic, and a
fake child proving bounded output retention.

**R12c. Remove Python soak execution and wire CI.**

`tests/run_all.sh` must always skip all three long scenarios and print exactly:

```text
harness soak --level <long|hour|overnight>
```

Remove the `RUN_LONG` Python path. PR-fast runs no soak; main runs long once;
nightly runs all levels three times and retains JSON. No shell option may run a
long/soak scenario through the Python generator.

Finally update `README.md`, `UNBLOCKING.md`, `docs/BLOCKERS.md`, protocol and
scenario docs, and this table to describe only verified present behavior.

**May run with:** none; this is the closeout packet.

## 7. Protocol Contract (Already Reserved; Do Not Renumber)

Node IDs: EVSE 6, OTA tool 7, telematics 8.

| ID | Name | Exact payload |
|---:|---|---|
| `0x203` | BMS_CHARGE_LIMIT | `[source=3, max_current_a_x2, soc, temp_c_x10_le16, fault, seq]` (7) |
| `0x610` | EVSE_EVENT | `[source=6, event, request_id, offered_current_a_x2, target_soc, reserved]` (6) |
| `0x611` | CHARGE_COMMAND | `[source=1, state, request_id, current_a_x2, target_soc, reason]` (6) |
| `0x630` | OTA_REQUEST | `[source=7, request_id, image_id, total_chunks]` (4) |
| `0x631` | OTA_CHUNK | `[source=7, request_id, chunk_index, data0..data4]` (8) |
| `0x632` | OTA_FINISH | `[source=7, request_id, total_chunks, crc32_le]` (7) |
| `0x633` | OTA_STATUS | `[source=1, request_id, state, status, active_slot, target_slot, reason, seq]` (8) |

EVSE events: 0 UNPLUG, 1 PLUG, 2 HANDSHAKE_OK, 3 STOP. Charging states:
0 DISCONNECTED, 1 PLUG_DETECTED, 2 HANDSHAKE, 3 ACTIVE, 4 LIMITED,
5 COMPLETE, 6 FAULT.

External actors are passive machines named `evse`, `ota_tool`, or
`test_harness`, attached to the sending bus. They use `[[bus_inject]]`.
Validation must require correct source byte, length, enum range, nonzero request
ID, ordered OTA chunks beginning at 0, matching finish count, PLUG before
HANDSHAKE, and OTA_REQUEST before chunks. Runtime gateway policy—not static
validation—decides whether frames after a rejected request are ignored.

Never add a scenario `mode` field.

## 8. Required Full Verification

During development run the focused packet gate first. Before declaring the
project complete run:

```bash
cd /home/zmm/projects/costar
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd /home/zmm/projects/microcar
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --bin microcar
./tests/run_all.sh
bash tests/verify_determinism.sh
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- toml-zoo
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- topology
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- diagnostics
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- charging
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- ota
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- cockpit
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- telematics
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- debug-gym
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- debug-gym-corpus
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- soak --level all --repeats 3
```

If repository waivers still make strict clippy impossible, the agent must cite
the exact existing waiver and show that its changed crates/files are warning
free. It may not add a new broad waiver.

## 9. Final Acceptance

Implementation is finished only when:

- two real Worlds and two concurrent real-firmware sessions match solo traces
  and isolate device/network ID 0;
- restart preserves only persistent devices, resets all volatile Rust/C/device
  state, resumes firmware, and enforces the downtime boundary;
- diagnostics, charging, OTA, cockpit, and telematics meet R6-R10 through real
  transport/device paths;
- all seven debug seeds and all typed predicate tests pass;
- all soak levels meet deterministic, retention, timeout, and RSS bounds;
- all legacy regressions and existing golden traces remain green; and
- the complete command block in Section 8 succeeds or has a narrow documented
  pre-existing waiver.

## 10. Prohibited Shortcuts

- Do not force vehicle mode or private gateway state from TOML.
- Do not count a trace marker as CAN delivery, display state, flash persistence,
  BMS propagation, or OTA recovery.
- Do not use process-global maps keyed by session ID for per-machine state.
- Do not use C thread-local storage for ECU instances.
- Do not let firmware call the plant directly.
- Do not let charging/OTA harnesses invoke private gateway helpers.
- Do not use host wall-clock ordering in deterministic network tests.
- Do not parse formatted human traces for typed predicates or authoritative
  semantic lane checks.
- Do not run long/soak scenarios through the Python trace generator.
- Do not delete or skip a red scaffold merely to make a lane count green.
- Do not remove a green legacy lane until its real replacement passes 100
  deterministic repetitions.
