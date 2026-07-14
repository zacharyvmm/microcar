# costar + microcar Dogfood — Status & Blockers Report

_Branch: `dogfood-milestone-2` on both repos. Updated after the 2026-07-13/14
remediation pass. Stage A / DeviceBank / execution-context foundation is now
substantially implemented and tested. B1 `sim_instance_state` is implemented
with zero-size/alignment regression tests. `NetworkBank` remains deferred.
Full Stage H host TCP telematics remains deferred. End-to-end dogfood lanes
are not complete unless explicitly marked complete in the table below.
`UNBLOCKING.md` remains the implementation contract for future work._

- costar: `github.com/zacharyvmm/costar` @ `2255db8` (PR #5)
- microcar: `github.com/zacharyvmm/microcar` @ `1b185c3` (PR #2)
- Host: Linux, Rust 1.97, workspace at `/home/zmm/projects`.

This document distinguishes completed product lanes from staged infrastructure.
The 2026-07-13/14 remediation pass delivered Stage A (DeviceBank isolation,
GuestRuntime, restart primitives, control-plane primitives, trace/predicate
foundations). M23-M27 P0a/P0b/P1 exit contracts are substantially met for
the foundation merge; remaining work is documented in `UNBLOCKING.md`.

---

## 0. Dogfood-plan implementation status (current session)

This section reflects the state after implementing `costar_microcar_dogfood_plan.md`.
It distinguishes **verified cores** (pure logic + unit tests, non-breaking,
existing golden traces byte-identical) from the **end-to-end lanes** in §16,
which additionally require firmware `main.c` integration + scenarios + harness
assertions and are not yet complete.

**CI gates green on both repos** (were red before): `cargo fmt --all --check`,
`cargo test --workspace`, `cargo build --bin microcar`. costar 433 tests,
microcar 260 tests, all passing. Full workspace clippy (`-D warnings`) is
intentionally waived for both repos (see MERGE_WAIVERS.md in each repo).

| Stage | Verified core delivered | End-to-end lane (§16) |
|---|---|---|
| A (gate) | **Complete + verified**: `Machine::with_device_context`/`configure_board`, `World::with_machine_devices`/`machine_ids`/`WorldError`; gRPC `machine_id` targeting + `Arc<Mutex<Session>>` map + resource bounds + terminal states; `PersistentDeviceState` + A3 restart algorithm; `control::drive_world` (both servers); A4 acceptance incl. `two_worlds_owned_can_interleave_100x`, `restart_downtime_delivery_boundary`, gRPC `concurrent_sessions_isolate_device_zero` (100×); A5 session-limit/keyframe/trace-ring/TTL tests | Residuals: gRPC `failed_session` test (needs firmware-injection hook), `serve.rs` `Arc<Mutex>` structural map refactor, microcar `b3` assertion extension |
| B1 | `sim_instance_state` allocator + `AlignedRegion` + `GuestRuntime` (15 tests, incl. zero-size/alignment regression) | `SIM_NOW`/`CURRENT_TASK_ID` migration onto the active runtime deferred (golden-risky) |
| B2/B3/B4 | — | 8-ECU C-global migration onto `sim_instance_state`; `NetworkBank`; isolation acceptance |
| C1 | protocol node/msg IDs, packed structs, EVSE/charge enums, `MC_DIAG_STALE`, Rust mirror, docs | BMS_STATUS byte-7 `seq` deferred (lands with D + regenerated goldens) |
| C2 | `validate_external_actor_frames` static validation (10 tests) | hostile scenarios + `harness charging`/`ota` runtime asserts |
| D | — | real diagnostics firmware (BMS seq + gateway cache + tool selectors) + Trace v2 causality harness |
| E1 | pure charging FSM C + Rust mirror (13 tests) | — |
| E2 | `BatteryModel::step_with_current` + i32 current + saturation (5 tests) | World plant CAN inbox + gateway/BMS/powertrain wiring + 5 scenarios |
| F1 | 32-byte OTA metadata record + CRC32 + `select`/`abort`/`recover_after_reset` C + Rust (5 tests) | — |
| F2/F3 | — | OTA tool ECU + gateway worker + 9 scenarios |
| G | pure dashboard framebuffer renderer C (per-screen pixel test) | cockpit wiring + FNV framebuffer hashes |
| H | telematics record parser C + Rust mirror (9 tests: byte-boundary, burst, errors) | telematics ECU firmware exists; `dogfood/src/telematics.rs` is a trace-based smoke test only, not the full Stage H host-TCP-bridge lane. True host-networking telematics remains deferred until `NetworkBank` is activated. |
| I1 | — | 3 debug seeds |
| I2 | typed `ContinuePredicate`/`DeviceCondition`/`ScalarValue` + evaluator + sinks (5 tests) | microcar semantic-event emission + end-to-end/replay tests |
| J1 | `TraceStats` accumulator + bounded retention (5 tests) | World/CLI wiring |
| J2/J3 | — | Rust soak harness + RSS bounds + CI wiring |

Also fixed pre-existing blockers unmasked while greening microcar: the
`microcar-plant` 4-vs-7-tuple test compile error, two stale examples
(`diag`, `boot_test`), several `clippy -D warnings` violations, and costar
`rustfmt` drift.

---

## 1. What is complete (M1-M22) and staged (M23-M27)

| Milestone | Track | Result |
|-----------|-------|--------|
| M1 | costar engine stabilization + microcar harness foundation | `run_until` tombstone/overshoot fix, fiber yielder, `sim_net_drain_tx` conservation, host-poller re-arm, TCP partial-write framing, SPI RX guard; dogfood harness crate |
| M2 | `simfarm` + `toml_zoo` lanes + hostile-input handling | Linux build fix (`fcntl` TAP fd); `microcar` never panics on bad input (exit 0/1/2); 11-case malformed corpus; concurrent-determinism/churn/panic-isolation |
| M3 | `topology` lane + CAN TX bus isolation | firmware CAN send routes only to the sender's buses |
| M4 | Trace v2 foundation (opt-in, byte-identical default) | `TraceV2` JSONL + correlation IDs + `to_human_line()` back-compat; `--trace-v2` |
| M5 | Gateway bus forwarding + parent/child causality | `[[bridge]]`, hop-based loop prevention, `parent_id` |
| M6 | Trace v2 full field set | `machine_id/name`, `component_id/type`, `port_id`, `payload_summary`, `task_id`, `rtos` |
| M7 | Debugging primitives | `World::step()` (run/run_until delegate → stepped==continuous by construction), `continue_until(predicate)` |
| M8 | `debug_gym` lane wired end-to-end | `microcar --step`; `harness debug-gym` asserts stepped==continuous, run_until-no-overshoot, clock-monotonic |
| M9 | `topology` lane complete **7/7** | `fleet_64_nodes` (64 machines/8 buses) + `bus_isolation_fault` (drop-fault isolates the forwarded frame) |
| M10 | Keyframe save/restore + deterministic replay | `World::replay_from_keyframe` (replay checkpoint, not stack snapshot) |
| M11 | Message breakpoint + `continue_until` fix | `World::run_to_frame`; fixed a predicate-miss on the `Done` step |
| M12 | Diagnostics dogfood lane | `harness diagnostics` 2/2: SERVICE mode, read mode, read/clear DTCs, actuator self-test, SERVICE torque clamp (trace-backed) |
| M13 | Charging dogfood lane | `harness charging` 1/1: plug→CHARGING, drive blocked while plugged, torque clamped to 0 (trace-backed, byte-identical) |
| M14 | Cockpit gRPC-surface lane (costar) | `sim-grpc/tests/cockpit_test.rs`: session/board/run(stream_display)/touch/inspect + framebuffer-hash & tick/end determinism |
| M15 | OTA happy-path dogfood lane | `harness ota` 1/1: `ota_state` IDLE→…→HEALTHY, crc-ok, boot healthy (trace-backed, byte-identical) |
| M16 | OTA slot-metadata model + rollback | pure C A/B-slot model + Rust mirror (7 tests); `gateway_ota_badcrc` rolls back; `harness ota` 2/2 |
| M17 | OTA fault-matrix extension | `gateway_ota_intwrite` + `gateway_ota_badhealth`; `harness ota` 4/4 (3 of 8 fault cases) |
| M18 | OTA commit-atomicity fault | `gateway_ota_powercut` (crc-ok yet rolled-back); `harness ota` 5/5 (4 of 8 fault cases) |
| M19 | `debug_gym` corpus #1 — OTA rollback bug | `gateway_ota_crcbug` vs `gateway_ota_badcrc`; `harness debug-gym-corpus` 1/1 |
| M20 | `debug_gym` corpus #2 — SERVICE torque-clamp bug | `powertrain_diag_service_bug` vs `powertrain_diag_service`; corpus 2/2 |
| M21 | `debug_gym` corpus #3 — clear-all-DTCs bug | `gateway_diag_clearbug` vs `gateway_diag_clear`; corpus 3/3 |
| M22 | `debug_gym` corpus #4 — START_SESSION-in-DRIVE bug | `gateway_diag_startdrivebug` vs `gateway_diag_startdrive`; corpus 4/4 |
| **M23** | **P0a staged — `DeviceBank`** | Device maps, fault injector, and IRQ state moved into a bank primitive. **Not complete:** its public raw-pointer context guard is unsafe under ordinary guard misuse, and no production World owns or activates a bank. |
| **M24** | **P0a staged — `SimulatorActivation`** | Opt-in owned-device activation has standalone tests. **Not complete:** no `Machine`, World, microcar, or gRPC production path opts in or provisions devices in the owned bank. |
| **M25** | **P0a staged — IRQ bank routing** | IRQ uses the same bank abstraction. It remains blocked on the safe active-context and real Machine ownership work. |
| **M26** | **P0b staged — receiver inbox** | A synthetic receiver-inbox test passes. **Not complete:** the World bridge still uses the default controller outside machine activation, and a second firmware-step path bypasses staging/draining. |
| **M27** | **P1 staged — factory and downtime fields** | A synthetic factory test passes. **Not complete:** microcar does not register factories, reset recreates default configuration, and frames received while down are replayed after boot. |

**Delivered plan tracks:** engine stabilization; networking/device-edge
hardening; `simfarm`; `toml_zoo` (9/11, 2 deferred); `topology` (7/7); Trace v2
data model; debugging primitives; the `debug_gym` determinism invariants; the
first diagnostics/charging/cockpit lanes; the OTA happy-path + slot model + 4 of
8 fault cases; and the first four `debug_gym` seeded-bug corpus cases (4/7).
M23-M27 are prerequisites-in-progress, not the completed chassis for the
remaining product work.

Test counts: costar `sim-world` **112**, `sim-devices` **132**, `sim-ffi` **21**,
`sim-core` 25, `sim-grpc` **15** (14 + cockpit, via
`PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc`); microcar `dogfood`
100, `state_tests` 99. All lanes green: `topology` 7/7, `toml-zoo` 11/11,
`simfarm` PASS, `debug-gym` 4/4 (unchanged hashes), `debug-gym-corpus` 4/4,
`diagnostics` 2/2, `charging` 1/1, `ota` 5/5. All 29 non-soak vehicle scenarios
(including the legacy restart scenarios) pass with golden traces intact. These
results do not exercise owned devices in a World, concurrent gRPC sessions, a
microcar firmware-factory reboot, or frame handling during reboot downtime.

---

## 2. Infrastructure Blockers — post-remediation status (2026-07-14)

The M23-M27 remediation pass substantially addressed P0a/P0b/P1:

- **B0 — active-context soundness.** `DeviceBank` activation was changed to a
  lifetime-safe scoped activation mechanism (`with_bank_if_active`). Guard tests
  now cover nested activation, out-of-order drop, forgotten guard, panic unwind,
  and IRQ scoping.
- **B1 — actual Machine and session ownership.** `device_registry!` accessors are
  routed through the active `DeviceBank`. Machines enable owned banks via
  `enable_owned_bank()`. gRPC board/touch/display accesses use machine-targeted
  sessions. Production World owns and activates banks.
- **B2 — CAN execution boundary.** Receiver-correct CAN inbox drains per-machine
  under the active device bank in `step_firmware`. The separate firmware-step
  bypass path is consolidated.
- **B3 — real restart semantics.** `FirmwareFactory` + `RestartSpec` preserve
  immutable machine/board configuration across reboots. `pending_boots` schedules
  deferred boots with nonzero downtime. Frames delivered while a target is
  stopped are dropped. `boot_at` delivery-boundary semantics (frames at exactly
  `boot_at` are delivered post-boot) are implemented with a regression test.

**Remaining deferred work:**
- `NetworkBank` / Ethernet isolation: not yet wired into `SimulatorExecutionContext`
- Full Stage H host TCP telematics: trace-smoke only; needs `NetworkBank`
- `SIM_NOW` / `CURRENT_TASK_ID` migration onto `GuestRuntime`: deferred (golden-risky)
- 8-ECU C-global migration onto `sim_instance_state`: deferred
- Concurrent duplicate-ECU gRPC session isolation: deferred

### Historical review snapshot: 2026-07-10

_The following section is preserved for reference. It describes the state
before the `dogfood-milestone-2` remediation pass._

The dependency chain remains **P0a → P0b → P1 → real firmware lanes**. M23-M27
do not retire that prefix; the following work is mandatory before a new lane is
called bus-backed, restart-backed, or session-isolated.

- **B0 — active-context soundness.** `DeviceBank` uses a public safe guard over
  a thread-local raw pointer. Non-LIFO drops or a forgotten guard can restore a
  dangling pointer. (Resolved in remediation: activation is now lifetime-safe.)
- **B1 — actual Machine and session ownership.** No production Machine enables
  an owned bank, and gRPC board/touch/display/inspection calls the fallback
  registry directly. (Resolved: `device_registry!` routed through active `DeviceBank`.)
- **B2 — CAN execution boundary.** The receiver inbox stages/drains outside the
  machine context. (Resolved: per-machine inbox draining in `step_firmware`.)
- **B3 — real restart semantics.** Microcar has no firmware factories, downtime
  frames are retained. (Resolved: `FirmwareFactory` + `pending_boots` + delivery-boundary.)
- **`protoc` is cleared.** The workspace compiler is present.

## 3. What remains, why, and the decision needed

Legend: **[BLOCKED]** = must wait for the M23-M27 remediation gate; **[DO]** =
implementable now with no product decision; **[PICK]** = implementable after its
dependencies with a modeling choice; **[SIGN-OFF]** = a later larger/risky change
that should be explicitly authorized.

### Track A — remaining `debug_gym` corpus seeds (3 of 7 left)
**Seeds left:** BMS stale sensor, dashboard missed warning, telematics
partial-write. Each needs a deliberately-buggy firmware variant in a subsystem
that has no dogfood firmware yet, plus its correct counterpart; the M19
`debug-gym-corpus` harness is reused unchanged.
- BMS stale sensor — **[PICK]** depends on the live-BMS snapshot shape (track C).
- Dashboard missed warning — **[PICK]** depends on the dashboard renderer (track E).
- Telematics partial-write — **[PICK]** depends on the telematics firmware (track D).
The gateway-bridge-loop seed is already covered structurally by the topology
`gateway_loop_prevention` scenario.
**Decision needed:** which subsystem to seed first (or "do all three, simple
defaults") after the relevant P0/P1 dependency is real. Do not use a
trace-backed substitute to claim the seed is complete.

### Track B — remaining OTA fault-matrix cases (4 of 8 left)
- **Gateway reset during update** — **[BLOCKED]** on B0-B3. The next agent must
  first wire the microcar gateway factory, preserve machine configuration and
  persistent storage, and prove that a nonzero-downtime reset reboots actual
  firmware rather than emitting reset markers. Then add the OTA scenario.
- **BMS critical fault during update** — **[BLOCKED]** on B1-B2. It requires
  receiver-correct CAN in an active machine-owned bank, not the current shared
  default-controller inbox test.
- **OTA-while-driving / OTA-while-charging** (and the deferred `toml_zoo`
  `charging-while-drive` / `ota-while-drive`) — **[DO].** *Previously* thought to
  need a `mode` field in the scenario schema. The durable approach (UNBLOCKING §4)
  is **not** a schema shortcut: model external actors (`evse`, `update_tool`) as
  passive named machines that `bus_inject` real protocol frames; the gateway then
  rejects OTA in DRIVE/CHARGING through its normal admission logic. This is P2
  (§ track G) and needs no new generic scenario field.
**Decision needed:** none for the architecture. The bus-backed/reset-backed
versions may be implemented only after B0-B3, and will replace rather than just
add to the trace-backed M16-M18 fixtures as product-grade coverage.

### Track C — deeper charging FSM + diagnostics live-BMS
- **Diagnostics live-BMS over real CAN** — **[BLOCKED]** on B1-B2. After the
  World-owned CAN path is real, the diagnostics-tool ECU sends `0x600`, the
  gateway caches the latest fresh BMS snapshot, and answers `0x601`.
  **Decision then needed:** snapshot fields (SOC / temperature / voltage /
  current), fixed-point scaling, and freshness window. A compact one-frame
  default is acceptable, but not before transport correctness is proven.
- **Charging FSM** (handshake → temperature-rise → reduced-current → complete,
  plus plant charge physics) — **[PICK].** **Decision:** how deep the FSM and how
  much plant physics. UNBLOCKING §5.2 says "deliberately simple is fine"; I can
  implement the compact `DISCONNECTED→PLUG_DETECTED→HANDSHAKE→ACTIVE→(LIMITED)→
  COMPLETE|FAULT` model with a fixed-point SOC/temperature/current-limit battery
  unless you want more fidelity.

### Track D — telematics lane
**[PICK] + largest new-firmware effort.** Needs a new telematics ECU (periodic
host uploads / remote queries over the existing Ethernet abstractions) and a
two-level lane: (1) deterministic virtual-network assertions, (2) a host-socket
integration test (loopback TCP, small buffers, fragmented reads/writes, transcript
+ conservation assertions). Host-networking engine edges are already hardened (M1).
**Decision:** the telematics application protocol shape (request-id framing,
record types) and whether to build the host-socket integration now or just the
deterministic virtual-network level first. Note: the host-socket level should
follow B0-B3 and the later clock/task-identity isolation (track F) if it will
run concurrently with other sessions in-process; standalone it is fine after
the relevant single-World ownership work is complete.

### Track E — cockpit follow-ups (first increment done, M14; `protoc` cleared)
**[PICK].** Remaining: (1) display-driving **dashboard firmware** so the
framebuffer has real, deterministic content (boot/drive/charging/OTA/warning
screens) instead of the honest empty-frame determinism M14 asserts; (2) reconcile
mode/warning events ↔ inspected device state ↔ framebuffer hashes in the cockpit
test; (3) a microcar `harness cockpit` wrapper (a small non-std gRPC client or a
shell-out — deferred to keep the dogfood harness std-only). A real concurrent
multi-session cockpit test is **[BLOCKED]** on B0-B2 and later clock/C-instance
isolation; the current cockpit proof remains sequential.
**Decision:** the dashboard screen content + touch semantics (touch must be a
harmless dashboard-local action, never bypassing vehicle safety authority). I can
pick sensible defaults.

### Track F — mandatory ownership/CAN/reboot remediation, then clock + C state
**[DO] for B0-B3; [SIGN-OFF] for the later concurrent duplicate-ECU expansion.**
The device layer is staged, not done. First implement the `UNBLOCKING.md`
remediation gate: safe active context, actual Machine/session ownership, one
CAN execution path, and end-to-end restart semantics. Only after those exit
tests pass does the remaining broader per-session refactor become:
- `SIM_NOW` / `CURRENT_TASK_ID` are still process-global atomics read by the C
  FFI during guest execution. Moving them into the per-World execution context
  (the M24 guard already scopes SimGlobal + DeviceBank; this would extend it to
  time + task identity) touches the FFI callback boundary and the C scheduler's
  assumptions — a subtle mistake corrupts every firmware scenario's timing/trace.
- The microcar C ECU files hold mutable statics / task handles / TCBs; one linked
  copy is shared by all instances of an ECU type, so two same-type ECUs (fleets,
  or concurrent gRPC sessions with duplicate ECUs) are not yet isolated. This
  needs an FFI-backed per-instance storage API (UNBLOCKING §1 "C firmware
  instance state").
- **Why the later stage is deferred:** it is required for *true concurrent
  in-process gRPC sessions with duplicate ECU types* and in-process fleets. The
  microcar binary is one World per process, and `simfarm` proves isolation across
  processes today. It remains a separate, high-risk move after B0-B3.
**Decision needed:** whether to prioritize that later clock/task-identity plus
C-instance-state stage. It is not permission to defer B0-B3.

### Track G — P2 protocol-backed scenario stimuli
**[DO] structurally; [BLOCKED] for product-grade bus assertions until B1-B2.**
Model external actors (`evse`, `update_tool`, `test_harness`) as passive
named machines with no firmware, attached to a bus, driving real protocol frames
via the existing `bus_inject` mechanism; add microcar-side validation
(sender attached to the named bus, payload source-id matches, event order legal,
cannot bypass gateway admission). This is the enabler for the mode-gated cases in
track B and the deferred `toml_zoo` cases, with no new generic scenario field.
**Decision needed:** none — the design is specified (UNBLOCKING §4). Confirm to
proceed.

### Track H — typed breakpoints + nightly/scale (P5)
**[DO], after consumers exist.** Predicates (vehicle_state, dtc_created,
device_state, assertion_failure) over structured events/snapshots, each with a
real consumer scenario; then a nightly composition (repeated deterministic lanes,
real corpus, topology/fleet scale, host telematics). Vehicle-state / DTC traces
already exist; device_state needs the cockpit dashboard (track E),
assertion_failure needs a semantic harness assertion event.
**Decision needed:** none; sequence it last.

---

## 4. The decisions actually needed from you

No product-modeling decision can bypass B0-B3. The remediation gate is required
implementation work, not a sign-off decision. The remaining inputs after that
gate are:

1. **Blanket authorization to pick "deliberately simple" defaults and proceed**
   for the modeling choices below (fastest path), **or** you specify each.
2. **Live-BMS snapshot shape** (track C): fields, fixed-point scaling, freshness
   window.
3. **Charging FSM depth + plant physics fidelity** (track C).
4. **Telematics application protocol** + whether to build the host-socket
   integration now or defer it behind the later clock/C-state stage (track D).
5. **Dashboard screen content + touch semantics** (track E).
6. **Concurrent in-process gRPC session isolation after B0-B3** (track F):
   prioritize the later clock/task-identity + C-instance-state move, or leave
   duplicate-ECU concurrency per-process for now. This later stage needs the
   genuine risk sign-off.
7. **Replace vs. keep** the trace-backed diagnostics/charging/OTA fixtures with
   their new real bus-backed/reset-backed equivalents (track B) — the old fixtures
   stay green as regressions during migration per UNBLOCKING's "do not remove a
   green lane until its real replacement has repeated-run stability."

Only B0-B3 and the structural portion of P2 are immediately actionable without
a product decision. The real OTA, diagnostics, charging, cockpit, and fleet
claims remain gated on their listed exit tests.

---

## 5. Recommended Order

1. **B0 context safety** — replace the unsafe active-bank lifetime mechanism
   and audit the matching `SimGlobal` mechanism. **[DO]**
2. **B1 Machine/session device ownership** — provision banks per machine and
   bind gRPC operations to an explicit session World and machine. **[DO]**
3. **B2 one CAN execution path** — centralize firmware stepping, RX staging,
   and sender TX draining in the active machine context; cover the former
   `advance_to` bypass. **[DO]**
4. **B3 restart semantics + microcar consumer** — preserve configuration and
   persistent state, drop downtime frames, wire firmware factories, and add the
   real gateway-reboot scenario. **[DO]**
5. **Track G (P2 external actors)** — add structural actors now if useful, then
   use them for semantic bus assertions after B1-B2. **[DO]**
6. **Track B and C real OTA/diagnostics/charging paths** — replace trace-backed
   fixtures only after the relevant exit tests are green. **[PICK/DO]**
7. **Track E dashboard, Track D telematics, Track A remaining corpus, and Track
   H predicates/nightly** — each after its real dependency exists. **[PICK/DO]**
8. **Later Track F clock/task/C-instance isolation** — only if concurrent
   in-process duplicate-ECU sessions are required. **[SIGN-OFF]**
