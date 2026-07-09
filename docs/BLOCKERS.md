# costar + microcar Dogfood — Status & Blockers Report

_Branch: `dogfood-milestone-1` on both repos. Updated after **M27** (P1
restartable machines). The **entire costar infrastructure foundation** the plan
was gated on — per-World device ownership (P0a), receiver-correct CAN (P0b), and
restartable machines (P1) — is now delivered and byte-identical, and the
`protoc`/gRPC tool gap is confirmed cleared. What remains is **firmware
authoring + a few product-modeling decisions**, not infrastructure sign-off._

- costar: `github.com/zacharyvmm/costar` @ `9265db2`
- microcar: `github.com/zacharyvmm/microcar` @ `d773982`
- Host: Linux, Rust 1.96.1, workspace at `/home/zmm/projects`.

This document explains **what is done**, **what remains**, **why**, and — in
detail — **exactly which decisions are still needed** to finish each remaining
track. The headline change since the last revision: the tracks previously marked
"blocked, needs sign-off" (the per-session/CAN refactor) and "blocked, needs a
tool" (`protoc`) are **cleared**. The remaining tracks are no longer
*infrastructure-blocked*; they are *implementable now*, most gated only on a
modeling choice that a product owner should confirm (or authorize "pick a
sensible default and proceed").

---

## 1. What is complete (27 milestones, verified locally)

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
| **M23** | **P0a device ownership — `DeviceBank`** | New `sim-devices/bank.rs`: all 15 device maps + `FaultInjector` moved into a per-World `DeviceBank` with an RAII active-context guard (`with_bank`), mirroring `sim-ffi`'s `with_sim_global`. Default-bank fallback ⇒ byte-identical. Two-world CAN-id-0 leakage test. sim-devices 126→131. |
| **M24** | **P0a — execution-context guard** | `Simulator::activate()` scopes an owned `DeviceBank` alongside `SimGlobal` (opt-in `enable_owned_devices`), restoring both on drop incl. panic. Nested-activation + panic-restore tests. Byte-identical by construction. sim-ffi 17→21. |
| **M25** | **P0a — IRQ controller into `DeviceBank`** | Last device-class singleton (`IRQ_CTRL`) migrated into the bank; `irq::with_irq[_mut]` route through `with_bank`. Every `sim-devices` store is now bank-owned. sim-devices 131→132. |
| **M26** | **P0b — receiver-correct CAN** | World-owned per-machine RX inboxes (`can_rx_inbox`) staged into controller 0 per firmware step; each ECU receives exactly its frames (no shared-queue cross-consumption). **Firmware CAN RX is now a reliable assertion path.** Proven byte-identical (disable-injection experiment left all 4 golden hashes unchanged) + `test_receiver_correct_can_no_cross_consumption`. sim-world 110→111. |
| **M27** | **P1 — restartable machines** | `FaultAction::Reboot` gains optional `downtime_ms` + a `Machine` `FirmwareFactory`: a restart recreates the *original* firmware and runs its boot path after a deterministic downtime, clears the P0b inbox, emits `machine_reset_begin`/`machine_reset_boot`. Legacy bare cold-boot preserved (no factory/downtime) ⇒ existing restart golden scenarios still PASS. sim-world 111→112. |

**Fully-delivered plan tracks:** engine stabilization; networking/device-edge
hardening; `simfarm`; `toml_zoo` (9/11, 2 deferred); `topology` (7/7); Trace v2
data model; debugging primitives; the `debug_gym` determinism invariants; the
first diagnostics/charging/cockpit lanes; the OTA happy-path + slot model + 4 of
8 fault cases; the first four `debug_gym` seeded-bug corpus cases (4/7); **and
now the full costar chassis the rest of the plan rests on — per-World device
ownership (P0a), receiver-correct CAN (P0b), and restartable machines (P1).**

Test counts: costar `sim-world` **112**, `sim-devices` **132**, `sim-ffi` **21**,
`sim-core` 25, `sim-grpc` **15** (14 + cockpit, via
`PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc`); microcar `dogfood`
100, `state_tests` 99. All lanes green: `topology` 7/7, `toml-zoo` 11/11,
`simfarm` PASS, `debug-gym` 4/4 (unchanged hashes), `debug-gym-corpus` 4/4,
`diagnostics` 2/2, `charging` 1/1, `ota` 5/5. All 29 non-soak vehicle scenarios
(incl. the three restart scenarios) pass with golden traces intact.

---

## 2. Foundation cleared this cycle (M23–M27) — what it changes

The plan's dependency chain was **P0a → P0b → P1 → (real firmware lanes)**. That
whole prefix is now done, which retires the two hardest blockers in the previous
revision of this document:

- **Old "track F — per-session state ownership refactor (highest risk, needs
  sign-off)" → the device half is DONE.** Virtual devices, the fault injector,
  and the IRQ controller are out of process/thread-global maps and into a
  per-World `DeviceBank` reached through an RAII execution-context guard (M23–M25).
  The C-FFI boundary is unchanged; golden traces are byte-identical. **What is
  left of track F** is narrower: `SIM_NOW` / `CURRENT_TASK_ID` and the C
  firmware-instance globals are still process-global, so *true concurrent
  in-process gRPC sessions* still share the clock/task identity. That residual is
  the only remaining piece of F (see §3, track F′).
- **Old "track E — `protoc`" → CLEARED.** The workspace compiler
  (`/home/zmm/projects/.tools/protoc-27.3/bin/protoc`, libprotoc 27.3) is present
  and working; `PROTOC=… cargo test -p sim-grpc` builds and passes all 15 tests.
- **CAN RX is now real (P0b).** Firmware on each ECU receives exactly the frames
  addressed to it. This retires the recurring caveat "firmware CAN RX is
  unreliable, so lanes must be trace-backed": product-grade **diagnostics /
  charging / OTA over real CAN** are now implementable, and a bus-backed lane can
  genuinely exercise producer→transport→consumer instead of a firmware-local
  trace hook.
- **A real reset primitive exists (P1).** `machine.reboot` with `downtime_ms` +
  a firmware factory recreates firmware and boots after downtime. The OTA
  "gateway reset during update" fault — previously blocked on "no reliable reset
  mechanism" — now has its mechanism.

Net: **no remaining item is blocked on a costar infrastructure gap or a missing
tool.** Everything below is either (a) firmware I can now author against a real
foundation, or (b) a small product-modeling decision, or (c) the one remaining
staged/risky costar refactor (clock + C-instance state) that is only needed for
concurrent in-process gRPC sessions.

---

## 3. What remains, why, and the decision needed

Legend: **[DO]** = implementable now with no decision (I can proceed); **[PICK]**
= implementable now but wants a modeling choice (I can pick a "deliberately
simple" default unless you specify); **[SIGN-OFF]** = a larger/risky change that
should be explicitly authorized.

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
defaults"). No infrastructure decision remains.

### Track B — remaining OTA fault-matrix cases (4 of 8 left)
- **Gateway reset during update** — **[DO].** *Previously blocked* on "no
  reliable reset mechanism." **Now unblocked** by P1 (`machine.reboot` +
  `downtime_ms` + firmware factory) and P0b (reliable CAN). Requires wiring a
  microcar firmware factory for the gateway and an OTA scenario that resets the
  gateway mid-update; the persistent A/B slot metadata (already a pure model,
  M16) must be read back on boot rather than living in RAM.
- **BMS critical fault during update** — **[DO].** *Previously blocked* on the
  CAN-RX path; **now unblocked** by P0b (the BMS can send a real over-CAN critical
  fault the gateway receives and aborts on).
- **OTA-while-driving / OTA-while-charging** (and the deferred `toml_zoo`
  `charging-while-drive` / `ota-while-drive`) — **[DO].** *Previously* thought to
  need a `mode` field in the scenario schema. The durable approach (UNBLOCKING §4)
  is **not** a schema shortcut: model external actors (`evse`, `update_tool`) as
  passive named machines that `bus_inject` real protocol frames; the gateway then
  rejects OTA in DRIVE/CHARGING through its normal admission logic. This is P2
  (§ track G) and needs no new generic scenario field.
**Decision needed:** none infrastructural. Confirm you want the real
bus-backed/reset-backed versions (they will *replace*, not just add to, the
trace-backed M16–M18 fixtures as the product-grade coverage).

### Track C — deeper charging FSM + diagnostics live-BMS
- **Diagnostics live-BMS over real CAN** — **[PICK].** Now doable over P0b: the
  diagnostics-tool ECU sends `0x600` requests, the gateway caches the latest
  fresh BMS snapshot and answers `0x601`. **Decision:** the BMS snapshot shape —
  which fields (SOC / temperature / voltage / current), their fixed-point
  scaling, and the freshness window. I can propose a compact one-CAN-frame
  default (e.g. SOC%, temp°C, pack mV/10, current signed A) unless you specify.
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
follow the remaining clock/task-identity isolation (track F′) if it will run
concurrently with other sessions in-process; standalone it is fine now.

### Track E — cockpit follow-ups (first increment done, M14; `protoc` cleared)
**[PICK].** Remaining: (1) display-driving **dashboard firmware** so the
framebuffer has real, deterministic content (boot/drive/charging/OTA/warning
screens) instead of the honest empty-frame determinism M14 asserts; (2) reconcile
mode/warning events ↔ inspected device state ↔ framebuffer hashes in the cockpit
test; (3) a microcar `harness cockpit` wrapper (a small non-std gRPC client or a
shell-out — deferred to keep the dogfood harness std-only). Concurrent
multi-session cockpit interlocks with track F′.
**Decision:** the dashboard screen content + touch semantics (touch must be a
harmless dashboard-local action, never bypassing vehicle safety authority). I can
pick sensible defaults.

### Track F′ — remaining per-session isolation: clock + C firmware-instance state
**[SIGN-OFF] — the only remaining genuinely-risky costar refactor.** The device
layer of the old track F is done (M23–M25). What is left:
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
- **Why it is deferred:** it is only required for *true concurrent in-process
  gRPC sessions with duplicate ECU types* and in-process fleets. The microcar
  binary is one World per process, and `simfarm` proves determinism/isolation
  across processes today. So this is high-risk for a benefit not needed by any
  current lane.
**Decision needed:** whether to prioritize concurrent in-process multi-session
isolation at all. If yes: explicit sign-off to take the clock/task-identity +
C-instance-state move in small verified stages (byte-identical at each step,
several iterations). If no near-term need: leave it and keep per-process
concurrency.

### Track G — P2 protocol-backed scenario stimuli
**[DO].** Model external actors (`evse`, `update_tool`, `test_harness`) as passive
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

Infrastructure sign-offs are **no longer required** (the foundation is done). The
remaining inputs are product-modeling choices. Concretely:

1. **Blanket authorization to pick "deliberately simple" defaults and proceed**
   for the modeling choices below (fastest path), **or** you specify each.
2. **Live-BMS snapshot shape** (track C): fields, fixed-point scaling, freshness
   window.
3. **Charging FSM depth + plant physics fidelity** (track C).
4. **Telematics application protocol** + whether to build the host-socket
   integration now or defer it behind track F′ (track D).
5. **Dashboard screen content + touch semantics** (track E).
6. **Concurrent in-process gRPC session isolation** (track F′): prioritize it
   (explicit sign-off for the staged clock/task-identity + C-instance-state move)
   or leave per-process concurrency as-is. This is the *only* remaining item that
   needs a genuine risk sign-off rather than a modeling choice.
7. **Replace vs. keep** the trace-backed diagnostics/charging/OTA fixtures with
   their new real bus-backed/reset-backed equivalents (track B) — the old fixtures
   stay green as regressions during migration per UNBLOCKING's "do not remove a
   green lane until its real replacement has repeated-run stability."

Everything marked **[DO]** in §3 (OTA gateway-reset + BMS-fault cases,
OTA/charging-while-drive via external actors, P2 stimuli, the P5 predicates) I can
implement now without any decision — they only need a "go."

---

## 5. Recommended order (given the foundation is done)

1. **Track G (P2 external actors)** — small, unblocks the mode-gated OTA/charging
   cases and the two deferred `toml_zoo` cases at once. **[DO]**
2. **Track B real OTA cases** — wire microcar gateway firmware factory + reset
   scenario (gateway-reset) and BMS over-CAN critical fault; plus
   OTA/charging-while-drive via track G. **[DO]** (uses P0b + P1)
3. **Track C diagnostics live-BMS over real CAN** — converts the first
   trace-backed lane to genuinely bus-backed; also seeds the BMS-stale corpus
   case (track A). **[PICK]** (BMS snapshot shape)
4. **Track C charging FSM + plant** — the richer EV model. **[PICK]**
5. **Track E dashboard/cockpit display firmware** — real framebuffer content;
   seeds the dashboard-missed-warning corpus case (track A). **[PICK]**
6. **Track D telematics** — new ECU + host-socket lane; seeds the
   telematics-partial-write corpus case (track A). **[PICK]**
7. **Track H predicates + nightly (P5)** — after the lanes above exist. **[DO]**
8. **Track F′ clock/task-identity + C-instance isolation** — last, only if
   concurrent in-process multi-session isolation is wanted. **[SIGN-OFF]**
