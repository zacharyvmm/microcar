# costar + microcar Dogfood: Remaining-Work Agent Guide

Last audited against the code: 2026-07-14, after `microcar#3` / `costar#6`.

This file is the execution contract for the next implementation PRs. It has been
trimmed so agents do **not** re-implement the R0/R1/R2/R3-core work already
landed in the current milestone-3 PRs. The next PR should start with the first
remaining packet in Section 6.

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
- deterministic telematics over virtual Ethernet and a host loopback TCP bridge,
  including fragmented reads and partial writes;
- seven reproducible debug-gym bugs and structured debugging predicates; and
- bounded-memory, repeatable CI at ordinary, concurrent, fleet, and eight-hour
  virtual-time scale.

A lane is not complete because it emits a desired trace label. Its producer,
transport/device, consumer, and observable effect must all execute, and the test
must inspect both semantic state and transport/device evidence.

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
communicate only through simulated devices/buses. The plant may communicate with
firmware only through its World-managed CAN endpoint.

## 3. How an Agent Must Use This Guide

1. Read the current code named by the packet. Do not copy an API from this
   document over an already-correct implementation.
2. Confirm every prerequisite packet is complete. If it is not, stop and take the
   prerequisite instead.
3. Preserve unrelated user changes and existing human golden traces.
4. Implement the smallest complete vertical slice described by the packet. Do not
   land half of a protocol lane behind unconditional fake trace output.
5. Run the packet's focused tests, then its stated regression gate.
6. Update the status table in Section 5 in the same change. Change a row to
   `DONE` only when all acceptance items pass.
7. Report changed files, observable behavior, exact verification commands, and
   any remaining failure. Do not report a stub, fixture, or pure model as an
   end-to-end lane.

Agents may work concurrently only on packets whose "May run with" entry says so.
Do not assign concurrent agents overlapping `sim-ffi/src/simulator.rs`,
`sim-world/src/world.rs`, `gateway_ecu/src/main.c`, or a single dogfood lane.

Use this assignment text verbatim when handing off a packet:

```text
Implement packet R<N> from docs/costar_microcar_dogfood_plan.md and no later
packet. Verify its prerequisites against the current code, preserve legacy
fixtures/goldens, meet every acceptance item, update the Section 5 status row,
and report changed files plus exact command results. Do not mark the packet
complete if an acceptance item is skipped or replaced by trace-only evidence.
```

For the next PR, use:

```text
Implement packet R6 from docs/costar_microcar_dogfood_plan.md and no later
packet. Verify the R0–R5 prerequisites against the current code, preserve legacy
fixtures/goldens, meet every acceptance item, update the Section 5 status row,
and report changed files plus exact command results. Do not mark R6 complete if
diagnostics values are not proven plant→BMS→gateway→tool over CAN with semantic
and transport evidence.
```

## 4. Definition of Done for Every Packet

Every packet must meet all applicable rules:

- Default behavior remains backward compatible; existing golden traces are
  byte-identical unless the packet explicitly replaces a golden and explains why.
- New behavior is enabled by a new scenario/firmware selection or a compatible
  optional API field.
- Tests assert typed state and real transport/device evidence. A local
  `sim_trace_*` record may aid diagnosis but cannot be the only assertion.
- Deterministic tests use virtual time and stable ordering, never host timing.
- Host-connected tests use loopback, bounded wall-time timeouts, byte/request
  conservation, and cleanup assertions rather than golden traces.
- No production path falls back to "the last active machine" or a process-wide
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
- `CORE ONLY`: reusable primitive exists; product integration or hardening is
  still absent.
- `SCAFFOLD`: files exist but are stubs, synthetic fixtures, or currently fail.
- `TODO`: implementation is absent.

| Area | Status | What is actually true |
|---|---|---|
| DeviceBank, explicit machine targeting, owned CAN, restart algorithm | DONE | `with_machine_devices`, owned banks, persistent-device snapshot/restore, boot boundary, gRPC machine IDs, and core acceptance tests exist. |
| Unified run control and gRPC session resource bounds | DONE | `drive_world`, per-session gRPC locks, deterministic listing, TTL, trace ring, and keyframe limits exist. |
| ECU role classification | DONE | `gateway_diag*` variants correctly classify as `gateway`; `diagnostics_tool_ecu` remains diagnostics; table-driven tests cover firmware variants. |
| Per-machine time and task identity | DONE | `GuestRuntime` accessors (`active_now`, `active_task_id`, etc.) are wired through C ABI paths; global atomics are legacy fallback only. |
| C firmware instance isolation | DONE | All eight ECUs use `sim_instance_state` with unique keys; mutable ECU state has moved into per-instance contexts. |
| NetworkBank core isolation | DONE | Per-machine NetworkBank is scoped via execution context; World Ethernet RX/TX for `ETH_DEVICES[0]` is banked and covered by 100x A/B World isolation tests; SimNetDevice and SmoltcpBridge objects are bank-local; destroy/recreate yields no stale core bank state; panic restores prior context. |
| NetworkBank host/TCP hardening | DONE | Host FD register/block/wake/deregister routes through the active `NetworkBank`; bank destroy/recreate clears fds/readiness/blocked task IDs; fragmented TcpBridge streams are isolated across banks; legacy TLS fallback remains for single-simulator paths. |
| JSON-RPC per-session locking | DONE | `sim-runner` uses `BTreeMap<u64, Arc<Mutex<Session>>>`; map lock is lookup-only; TTL exempts Running/Paused; cleanup ≤30s + create/list; Stop → Done; session-limit / TTL-exempt / failed-sibling tests pass. |
| Restart/control-plane residuals (R4) | DONE | gRPC `failed_session_returns_world_and_sibling_runs` with panicking test firmware; JSON-RPC sibling Error isolation test; `b3_gateway_reboot_downtime` + `tests/gateway_reboot_downtime.rs` (100× flash/RTOS/volatile/sibling). |
| Duplicate-world/session isolation gate (R5) | DONE | `two_worlds_owned_can_interleave_100x`; microcar Trace v2 recreate hashes 100×; BMS plant-frame isolation; gateway `boot_at` CAN delivery; gRPC dual-session real-firmware configure with colliding device IDs 100× (`tests/r5_isolation.rs`). |
| Protocol IDs/payload structs and external-actor validation | DONE | Stage C IDs and packed structs exist; validation has tests. |
| Live diagnostics | SCAFFOLD | BMS now consumes `MC_MSG_PLANT_SENSORS` (0x500) into live state; gateway still returns `UNSUPPORTED` for live BMS selectors; diagnostics tool does not yet issue/assemble protocol requests. |
| Charging | CORE ONLY | C FSM and battery signed-current method exist. Rust FSM mirror is a one-test stub; firmware and plant CAN loop are not wired. |
| OTA persistence model | CORE ONLY | The 32-byte metadata model and Rust mirror exist. Gateway still runs an in-RAM timed script, and OTA tool uses incompatible local message definitions. |
| Dashboard | CORE ONLY | C renderer and FreeRTOS display task exist. The authoritative gRPC test uses synthetic Rust firmware, not microcar firmware; Zephyr and product-session coverage are incomplete. |
| Telematics | SCAFFOLD | ECU sends a custom periodic record. C parser and Rust mirror are TODO stubs; current harness is a trace/determinism smoke test only. |
| Typed predicates | SCAFFOLD | Types and semantic storage exist, but `Device` always returns `false`; firmware events are not connected to the World; end-to-end/replay tests are absent. |
| TraceStats and soak | CORE ONLY | A line-based accumulator exists in `sim-core`; it is not wired to World/CLI, and no Rust soak harness exists. |
| Debug corpus | SCAFFOLD | Four seeds are registered and should pass after the R0 classification fix. Three additional directories exist but are not real or registered. |

Known command results must be refreshed by the next agent. At this audit point,
the milestone-3 PRs claim green CI for the focused infrastructure and passing
legacy debug-gym corpus after R0. The product lanes remain intentionally red or
scaffolded until their packets land.

Many files under `dogfood/diagnostics`, `dogfood/charging`, `dogfood/ota`, and
`dogfood/debug_gym` are specifications-in-TOML, not completed scenarios. In
particular, `sender = "plant"` is invalid because there is no passive machine
named `plant`; final scenarios must consume sensor frames published by the real
`[plant]`, not repair this by inventing a fake plant sender.

## 6. Remaining Work Packet Order

Completed and removed from the implementation queue:

```text
R0 ECU classification
R1 GuestRuntime time/task identity
R2 per-instance C firmware state
R3 core NetworkBank activation and Ethernet device-0 isolation
R3H host/TCP NetworkBank hardening
R4 control-plane and restart residuals
R5 duplicate-world/session isolation gate
```

Remaining order:

```text
R6 diagnostics
R7 charging  (may run with R6 if gateway_ecu ownership is coordinated)
R8 telematics (may run with R6/R7)
R6 + R7 ───────────────── R9 OTA
R5 + R6/R7/R9 as inputs ─ R10 cockpit
R6 + R8 + R10 ─────────── R11 debug corpus + predicates
R6..R11 ───────────────── R12 soak + CI + final documentation
```

R6 and R7 may run concurrently after R5 if agents coordinate ownership of
`gateway_ecu/src/main.c`; otherwise run them sequentially. R8 may run in parallel
with R6/R7 because it owns the network and telematics surfaces. R9 must follow
both R6 and R7 because OTA admission depends on fresh BMS state and charging
state. R10 should follow the real vehicle states it displays.

### R3H — Harden NetworkBank host FD and TCP stream isolation

**Status: DONE** (2026-07-17)

**Goal:** network device 0, TCP framing, and host poller state are isolated and
destroyed with their owning machine/session, not merely the in-process Ethernet
and smoltcp-core objects.

**Files:**

- `costar/crates/sim-net/src/bank.rs`
- `costar/crates/sim-net/src/lib.rs`
- `costar/crates/sim-net/src/host_poller.rs`
- `costar/crates/sim-net/src/tcp_bridge.rs`
- `costar/crates/sim-net/src/tap_bridge.rs`
- `costar/crates/sim-ffi/src/net_ffi.rs`
- reset/clone/rebuild/destruction paths in gRPC and JSON-RPC sessions if needed

**Prerequisites already complete:** R0, R1, R2, and R3 core NetworkBank activation.
Do not re-implement them.

**Implementation:**

1. Route `sim_host_register_fd`, `sim_host_deregister_fd`, and
   `sim_host_block_on_fd` through bank-aware host-poller accessors. When a
   `NetworkBank` is active, fd registration, readiness, blocked task IDs, and
   cleanup must live in that bank rather than the legacy thread-local
   `HOST_POLLER`.
2. Add explicit host-poller lifecycle APIs on `NetworkBank` if needed. A bank
   destroy/reset/clone/keyframe rebuild must not retain registered handles,
   stale readiness, or blocked task IDs from the old bank.
3. Add a Unix-only destroy/recreate test proving no stale fd readiness, blocked
   task ID, or registered handle survives bank destruction. Use loopback or a
   controlled fd pair; do not depend on external network services.
4. Add a real fragmented TCP/stream isolation test using `TcpBridge` partial
   read/write buffers or smoltcp socket state with partial TCP payloads. Two
   banks/sessions using the same logical network IDs must receive only their own
   byte streams.
5. Keep the legacy no-active-bank fallback for single-simulator compatibility.
   Do not let the fallback mask missing active-bank routing in production World
   or server paths.
6. Update the status table only when host FD and fragmented TCP isolation tests
   both pass.

**Acceptance:**

- Host fd register/block/wake/deregister uses the active `NetworkBank` whenever
  one is active.
- Destroying and recreating a bank cannot leak stale fd readiness, registered fd
  handles, blocked task IDs, TCP bridge buffers, or TAP bridge registrations.
- Two concurrently banked TCP/stream paths with fragmented/partial payloads do
  not leak, reorder, duplicate, or drop bytes across banks.
- Existing R3 core tests still pass: Ethernet device-0 RX isolation, SimNetDevice
  queue isolation, SmoltcpBridge isolation, bank destroy/recreate, and panic
  restoration.
- Legacy direct single-simulator tests still pass through fallback mode.

**Focused gate:**

```bash
cd /home/zmm/projects/costar
cargo fmt --check
cargo clippy -p sim-net -p sim-ffi --all-targets -- -D warnings
cargo test -p sim-net -p sim-ffi
cargo test -p sim-world two_worlds_eth_device_zero_rx_isolated_100x
```

**Regression gate:**

```bash
cd /home/zmm/projects/costar
cargo test --workspace
```

**May run with:** none; this packet touches shared network execution-context and
host-poller code.

### R4 — Finish control-plane and restart residuals

**Status: DONE** (2026-07-16)

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

1. Change JSON-RPC storage to `Mutex<BTreeMap<u64, Arc<Mutex<Session>>>>`. Hold
   the map lock only for lookup/insert/remove; lock one session for operations.
   Run uses the existing take/return-World pattern and `drive_world`.
2. Every success, stop, disconnect, error, and panic path must return the World
   or mark the session `Error`. Natural completion/Stop => `Done`; disconnect =>
   `Paused`; simulation error/panic => `Error`.
3. Add the missing gRPC `failed_session_returns_world_and_sibling_runs` test.
   Register a test firmware factory that deliberately errors/panics; do not add a
   product-only panic hook.
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

**Status: DONE** (2026-07-17)

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
dev-dependencies on `sim-grpc`, `tokio`, and `tonic`. Keep the `microcar-dogfood`
library std-only. Generic costar tests must use generic test firmware and must
not depend on microcar.

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
- `microcar/common/src/microcar_protocol.*`
- `microcar/dogfood/diagnostics/*`
- `microcar/src/validate.rs` and harness assertion code if needed

**Implementation:**

1. Fix the BMS input/dispatch mismatch so plant/BMS status frames are consumed by
   the BMS path that updates sequenced live state.
2. Gateway must serve live BMS diagnostic selectors from fresh BMS state instead
   of returning `UNSUPPORTED`.
3. Diagnostics tool must issue requests and assemble responses using protocol
   frames and request IDs, not direct gateway mutation or timed trace scripts.
4. Scenarios must assert freshness, request/response correlation, timeout/stale
   rejection, and no cross-session contamination.
5. Keep the legacy trace-backed diagnostics fixtures as regressions until the
   protocol-backed lane passes 100 repetitions.

**Acceptance:**

- Diagnostics values are produced by plant/BMS, transported over CAN, consumed by
  gateway, returned over CAN, and observed by the diagnostics tool.
- Tests inspect semantic state and CAN/device evidence, not only trace labels.
- The diagnostics harness passes 100 repeated runs.
- Existing debug-gym diagnostics seeds still pass.

**May run with:** R7 only if agents coordinate `gateway_ecu/src/main.c`; otherwise
run sequentially.

### R7 — Real charging plant/firmware CAN loop

**Prerequisite:** R5.

**Goal:** charging behavior is driven by plant and firmware CAN frames, not by a
trace script forcing gateway state.

**Files:**

- `microcar/plant/*`
- `microcar/firmware/gateway_ecu/src/main.c`
- `microcar/firmware/powertrain_ecu/src/main.c`
- `microcar/firmware/bms_ecu/src/main.c`
- charging scenarios and harness assertions

**Implementation:**

1. Wire charger plug/current/voltage state from the plant into CAN frames that
   firmware consumes.
2. Gateway and powertrain must enter/exit charging through real messages and
   state machines.
3. Battery SOC/current behavior must use signed current correctly and enforce the
   no-drive-while-charging rule through firmware behavior.
4. Replace trace-only charging fixtures with protocol-backed tests while keeping
   legacy fixtures as regressions until the new lane passes repeatedly.

**Acceptance:**

- Charging state flows through plant -> BMS/gateway/powertrain over CAN.
- Drive is blocked while charging by firmware-observable state.
- SOC/current evolves deterministically and is asserted semantically.
- Charging harness passes 100 repeated runs.

**May run with:** R6 only if agents coordinate `gateway_ecu/src/main.c`; otherwise
run sequentially.

### R8 — Real telematics over virtual Ethernet and host TCP

**Prerequisite:** R5 and R3H.

**Goal:** telematics records are parsed and verified through deterministic
virtual Ethernet and host loopback TCP paths, including fragmentation.

**Files:**

- `microcar/firmware/telematics_ecu/src/main.c`
- `microcar/common/src/microcar_telematics.*`
- `microcar/dogfood/telematics/*`
- `costar` network APIs only if R3H left a missing generic primitive

**Implementation:**

1. Finish the C parser and Rust mirror for length-prefixed telematics records.
2. Add deterministic virtual-Ethernet tests for complete and fragmented records.
3. Add host loopback TCP tests with partial reads/writes, byte conservation, and
   cleanup assertions.
4. Ensure duplicate sessions using device/network ID 0 do not leak telematics
   records across sessions.

**Acceptance:**

- Telematics harness proves record parse, sequence, freshness, fragmentation, and
  isolation.
- Tests assert bytes and semantic decoded records, not only trace smoke output.
- Host-connected tests use bounded wall-time and cleanup checks.

**May run with:** R6/R7 after R5, because it owns the network/telematics surfaces.

### R9 — OTA through protocol and persistent storage

**Prerequisites:** R6 and R7.

**Goal:** OTA admission, transfer, commit, reboot, health check, and rollback are
protocol-driven and persist through the simulated storage model.

**Files:**

- `microcar/firmware/gateway_ecu/src/main.c`
- `microcar/firmware/ota_tool_ecu/src/main.c`
- `microcar/common/src/microcar_ota_slot.*`
- OTA scenarios and harness assertions

**Implementation:**

1. Replace the gateway in-RAM timed OTA script with protocol frames from the OTA
   tool.
2. Use the persistent metadata model for slot state across reboot.
3. Gate OTA admission on fresh diagnostics/BMS/charging state from R6/R7.
4. Complete bad CRC, interrupted write, bad health, and powercut-precommit cases.

**Acceptance:**

- OTA state persists across reboot where required and volatile state resets.
- Rollback cases are proved through protocol/storage state, not trace scripts.
- OTA harness passes 100 repeated runs.

**May run with:** none; depends on R6 and R7 state.

### R10 — Real cockpit/dashboard product session

**Prerequisites:** R5 and the vehicle states it displays, especially R6/R7/R9.

**Goal:** cockpit tests use real microcar firmware and prove display/touch/session
behavior, not synthetic Rust firmware only.

**Files:**

- dashboard firmware and renderer files
- `costar/crates/sim-grpc/tests/*`
- `microcar/tests/*`
- cockpit/dashboard scenarios and assertions

**Implementation:**

1. Load real dashboard firmware in the authoritative gRPC cockpit test.
2. Configure display/touch/timer/ADC/CAN/Ethernet IDs that collide across two
   sessions and prove isolation.
3. Assert framebuffer contents/dirty rects and harmless dashboard-local touch
   behavior.
4. Include Zephyr dashboard coverage or explicitly keep it marked incomplete.

**Acceptance:**

- Product session streams display frames with correct `machine_id` and device ID.
- Touch injection targets the requested machine and cannot affect sibling
  sessions.
- Real dashboard firmware renders meaningful state from real vehicle messages.

**May run with:** none unless split into clearly non-overlapping dashboard vs gRPC
subtasks.

### R11 — Finish debug corpus and typed predicates

**Prerequisites:** R6, R8, and R10.

**Goal:** the seven debug-gym bugs are reproducible and typed predicates can be
used end-to-end and through replay.

**Files:**

- `microcar/dogfood/debug_gym/*`
- predicate type/storage/evaluation code in `costar` and `microcar`
- firmware event wiring and replay tests

**Implementation:**

1. Convert the three placeholder debug-gym directories into real registered
   scenarios.
2. Wire firmware/device/world events into typed predicate storage.
3. Replace `Device` predicate's unconditional `false` with real evaluation.
4. Add end-to-end and replay tests proving predicate stability.

**Acceptance:**

- Seven debug-gym seeds are registered and pass/fail as expected.
- Typed predicates observe real events and survive replay.
- The corpus is deterministic across repeated runs.

**May run with:** none; depends on product lanes.

### R12 — Soak, fleet, CI, and final documentation

**Prerequisites:** R6 through R11.

**Goal:** prove the completed dogfood behaves deterministically at ordinary,
concurrent, fleet, and long virtual-time scale.

**Files:**

- CI workflows
- soak/fleet harness code
- final documentation and status tables

**Implementation:**

1. Wire TraceStats into World/CLI output and retain bounded memory behavior.
2. Add Rust soak harnesses for ordinary, concurrent, fleet, and eight-hour
   virtual-time scale.
3. Add CI gates for the completed product lanes and debug corpus.
4. Update docs so every `DONE` row corresponds to executable tests.

**Acceptance:**

- Soak/fleet tests are deterministic and bounded.
- CI covers the final product lanes without relying on local-only scripts.
- Final docs accurately distinguish shipped behavior from future work.

**May run with:** none; this is the final closeout packet.
