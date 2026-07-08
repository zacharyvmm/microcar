# costar + microcar Dogfood — Status & Blockers Report

_Branch: `dogfood-milestone-1` on both repos. Updated after the M17 OTA
fault-matrix extension and the M18 OTA commit-atomicity (power-cut) fault case._

- costar: `github.com/zacharyvmm/costar` @ `0c63a26`
- microcar: `github.com/zacharyvmm/microcar` @ `a6dc9a4`
- Host: Linux, Rust 1.96.1, workspace at `/home/zmm/projects`.

This document explains **what is done**, **what remains**, and — in detail — **why each
remaining track is blocked** and exactly what input/decision is needed to unblock it.

---

## 1. What is complete (18 milestones, verified locally)

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
| M12 | Diagnostics dogfood lane | `harness diagnostics` 2/2: SERVICE mode, read mode, read/clear DTCs, actuator self-test, SERVICE torque clamp |
| M13 | Charging dogfood lane | `harness charging` 1/1: plug→CHARGING, drive blocked while plugged, powertrain torque clamped to 0 (trace-backed, golden traces byte-identical) |
| M14 | Cockpit gRPC-surface lane (costar) | `crates/sim-grpc/tests/cockpit_test.rs`: session/board(display+touch+timer+adc)/run(stream_display)/touch/inspect + framebuffer-hash & tick/end determinism; test-only, golden traces unaffected |
| M15 | OTA happy-path dogfood lane | `harness ota` 1/1: `ota_state` IDLE→…→HEALTHY, crc-ok, boot healthy (trace-backed, opt-in, golden traces byte-identical) |
| M16 | OTA slot-metadata model + rollback | pure C A/B-slot model (`common/microcar_ota_slot.c`) + Rust mirror (7 tests); `gateway_ota_badcrc` fault variant → corrupt image rolls back to slot A; `harness ota` **2/2** (opt-in, golden traces byte-identical) |
| M17 | OTA fault-matrix extension | `gateway_ota_intwrite` (power-cut/interrupted write → rollback before verify) + `gateway_ota_badhealth` (valid image commits then fails self-test → rollback); shared `emit_ota_rollback()`; `harness ota` **4/4** (3 of 8 fault cases; opt-in, golden traces byte-identical) |
| M18 | OTA commit-atomicity fault | `gateway_ota_powercut` — a valid, verified image is discarded by a power cut *before* the atomic commit and reverts to slot A (crc-ok yet rolled-back, distinguishing it from bad-CRC); `harness ota` **5/5** (4 of 8 fault cases; opt-in, golden traces byte-identical) |

**Fully-delivered plan tracks:** engine stabilization; networking/device-edge hardening;
`simfarm`; `toml_zoo`; `topology` (7/7); Trace v2 data model; debugging primitives
(`step`, `continue_until`, keyframe replay, message breakpoint), the `debug_gym`
determinism invariants, the first diagnostics lane, the first charging safety lane,
the first cockpit gRPC-surface lane, the OTA happy-path lane, and the OTA
slot-metadata model + first four fault-matrix (rollback) cases.

Test counts: costar `sim-world` 110 unit tests, `sim-core` 25, `sim-grpc` 15 integration
tests (14 + 1 new cockpit); microcar `dogfood` 83 unit tests, `state_tests` 99 unit tests.
All lanes green: `harness topology` 7/7, `toml-zoo` 11/11, `simfarm` PASS, `debug-gym` 4/4
(unchanged hashes), `diagnostics` 2/2, `charging` 1/1, `ota` 5/5. All 29 non-soak vehicle
scenarios pass with golden traces intact.

---

## 2. Remaining work and its blockers

Every remaining blocked plan item is blocked on one of two things: **new firmware** or a
**large/risky costar refactor** that needs sign-off. The previous `protoc` blocker for the
gRPC/cockpit lane has been unblocked locally; see 2.3.

### 2.1 Firmware EV lanes — `charging`, `ota`, remaining diagnostics depth  ⟶ PARTIALLY BLOCKED

**What the plan wants:** a vehicle-mode state machine adding `CHARGING`, `SERVICE`,
`OTA_UPDATE` (and `TRANSPORT_MODE`), plus ECU firmware behavior:
- diagnostics: diagnostic session, read vehicle mode, read/clear DTCs, live BMS data,
  actuator self-test, "service mode disables drive".
- charging: plug/handshake/charging/temperature-rise/reduced-current/complete, "drive
  blocked while plugged".
- ota: download → write slot B → CRC → commit → reboot → health check → rollback, plus an
  8-case power-cut/corruption/reset fault matrix.

**Diagnostics status:** unblocked for dogfood. The repo now has protocol support for
`SERVICE`, `OTA_UPDATE`, `TRANSPORT_MODE`, diagnostic request/response IDs
(`0x600`/`0x601`), gateway DTC/session handling, a diagnostics tool ECU, service-mode torque
blocking, and a `harness diagnostics` lane with two scenarios. Verified locally:
`MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- diagnostics`
passes 2/2.

**Remaining blockers:**
1. **Charging: first safety lane delivered (M13).** The "drive blocked while plugged" safety
   contract now has a green `harness charging` lane, using trace-backed charging dogfood
   firmware variants (`gateway_charging`, `powertrain_charging`) in the same pattern as
   diagnostics. Still missing: the richer charging FSM (handshake / temperature-rise /
   reduced-current / charge-complete) and the charging plant physics. **OTA: happy-path
   lane (M15) + slot-metadata model & first rollback fault (M16) delivered.** A trace-backed
   `gateway_ota` dogfood variant drives the minimal happy-path state sequence
   (IDLE→DOWNLOADING→VERIFYING→COMMIT_PENDING→REBOOTING→HEALTHY with crc-ok / slot-B /
   boot-healthy markers), and M16 added a pure A/B **slot-metadata model**
   (`common/microcar_ota_slot.c` + a 7-test Rust mirror) that the firmware now drives, plus
   a `gateway_ota_badcrc` fault variant whose corrupt image fails CRC and **rolls back** to
   the known-good slot A. M17 added two more fault-matrix cases reusing the same model:
   `gateway_ota_intwrite` (power-cut / interrupted write → rollback before verify) and
   `gateway_ota_badhealth` (a valid image commits then fails its post-reboot self-test →
   rollback to slot A), and M18 added `gateway_ota_powercut` (a valid, verified image is
   discarded by a power cut *before* the atomic commit → revert to slot A, proving commit
   atomicity). `harness ota` is **5/5** green (opt-in, golden traces byte-identical),
   covering 4 of the plan's 8 fault cases. Still missing (all decision-gated, deliberately
   not guessed autonomously): **gateway reset during update** and **BMS critical fault during
   update** need a reliable cross-ECU / reset mechanism to model honestly (the CAN-RX path is
   still unreliable — a trace-only stub would be fabricated, not genuinely exercised); and the
   two mode-gated cases (**OTA-while-driving** / **OTA-while-charging**) need OTA in the
   scenario schema. Also still open: OTA over real firmware CAN.
2. The plan's deeper diagnostics item, live BMS data, is not implemented yet.
3. A simulator integration gap remains: firmware-originated CAN and firmware RX are not a
   reliable assertion path in these diagnostics scenarios, so the diagnostics lane uses
   explicit dogfood firmware variants (`gateway_diag_*`, `powertrain_diag_service`) and
   firmware trace events. A product-grade diagnostics-over-bus lane should wait for the
   per-session/device ownership work or a narrower CAN RX/TX fix.
4. The two deferred `toml_zoo` cases (`charging-while-drive`, `ota-while-drive`) are still
   waiting on charging/OTA modes existing in the scenario schema.

**To unblock the remaining EV lanes:** approve the charging/OTA firmware track and confirm
the modeling approach (charging FSM depth, OTA slot model, and whether diagnostics live BMS
must be over simulated CAN or may use trace-backed firmware hooks until the CAN RX/TX issue
is fixed).

### 2.2 `debug_gym` seeded-bug corpus  ⟶ 4 OF 7 CASES DELIVERED (M19–M22); rest firmware-gated

**What the plan wants:** 7 seeded bugs (gateway race, powertrain timeout/cancel, BMS stale
sensor, dashboard missed warning, telematics partial-write, OTA rollback, gateway bridge
loop), each with: description, expected symptom, minimal failing scenario, **golden
failing trace**, required debugging primitive, and **fixed trace**.

**Delivered (M19–M22):** the first four corpus cases are done via
`dogfood/src/debug_gym_corpus.rs` + `harness debug-gym-corpus` (**4/4** green):
(1) **OTA rollback bug** — a broken CRC check (`gateway_ota_crcbug`) accepts a corrupt image
and boots the bad slot, vs the M16 `gateway_ota_badcrc` that rolls back; (2) **SERVICE-mode
torque-clamp bug** — a powertrain that skips the SERVICE safety clamp
(`powertrain_diag_service_bug`) commands drive torque during a service session, vs the M12
`powertrain_diag_service` that clamps to 0 / disables the motor; (3) **clear-all-DTCs bug** —
a gateway that calls `fault_manager_clear_all` on a BMS-scoped CLEAR_DTCS
(`gateway_diag_clearbug`) silently drops an unrelated powertrain DTC (2 DTCs before, 0
after), vs `gateway_diag_clear` that clears only the BMS node (2 before, 1 after);
(4) **START_SESSION-in-DRIVE bug** — a gateway that skips the safety guard refusing a
diagnostic session while driving (`gateway_diag_startdrivebug`) accepts START_SESSION
mid-drive (status=OK, mode=SERVICE), vs `gateway_diag_startdrive` that rejects it
(status=REJECTED, mode=DRIVE). Cases (2)–(4) are the Strategy A diagnostics seeds. Each seed
carries the plan's required metadata and the harness runs both the **failing** (buggy) and
**fixed** scenarios through the product binary, asserting the bug reproduces (golden failing
trace), the fix resolves it (fixed trace), and the two traces diverge at the documented
point — exactly what the seed's debugging primitive localizes. Genuinely exercised (real
buggy firmware, real traces), gated opt-in, golden traces byte-identical.

**Why the rest is still blocked:** the debug_gym *primitives* are done (M7/M8/M10/M11: step,
continue_until, keyframe replay, message breakpoint, and the determinism invariants), and
the corpus *harness* now exists. All diagnostics/OTA-seeded cases reachable without new
firmware are done. The remaining 3 seeds — **BMS stale sensor**, **dashboard missed
warning**, and **telematics partial-write** — still need their own deliberately-buggy
**firmware variants** to produce the golden failing/fixed traces (telematics also needs the
telematics firmware from §2.5). The gateway-bridge-loop bug is already exercised structurally
by the topology `gateway_loop_prevention` scenario.

**To unblock the remaining seeds:** add the next buggy firmware variants (diagnostics-seeded
first, telematics later) — each reuses the M19 `debug-gym-corpus` harness.

### 2.3 `cockpit` lane + gRPC control plane  ⟶ FIRST INCREMENT DELIVERED (M14)

**What the plan wants:** dogfood the GUI-facing path — `CreateSession`, `ConfigureBoard`
(display + touch + timer + ADC), `Run(stream_display=true)`, `InjectTouch`,
`InspectDevices`, framebuffer-hash assertions, trace↔device reconciliation. This is the
`sim-grpc` product surface.

**Previous blocker (cleared):** `sim-grpc`'s `build.rs` compiles `.proto` files via
`tonic-build`, which requires the `protoc` protobuf compiler. A workspace-local official
compiler is installed at `/home/zmm/projects/.tools/protoc-27.3/bin/protoc`; export
`PROTOC` when building/testing costar.

**Delivered (M14):** `crates/sim-grpc/tests/cockpit_test.rs` — an additive, test-only
integration test (`cockpit_grpc_surface_and_determinism`) that runs the full cockpit flow
end-to-end (`CreateSession → LoadScenario → ConfigureBoard(display+touch+timer+adc,
n=4) → Run(stream_display,stream_trace) + touch press/release + Stop → InspectDevices`),
reconciles the inspected devices against `ConfigureBoard`, and asserts framebuffer-hash +
tick-boundary + `SimulationEnd`-totals determinism across two **sequential** runs. Verified:
`PROTOC=… cargo test -p sim-grpc` → 14 existing + 1 new cockpit test all pass. No server
change ⇒ golden traces unaffected.

**Remaining (follow-ups, not blocking):**
1. A microcar `harness cockpit` wrapper — deferred to keep the dogfood harness std-only
   (would need a gRPC client or a shell-out into the costar test).
2. Rich framebuffer-content assertions (Strategy B) — need display-driving dashboard
   firmware; the current scenarios emit zero DisplayFrame events (empty-frame determinism
   is asserted honestly, no fabricated pixels).
3. Dashboard-state↔trace reconciliation and concurrent multi-session isolation — the
   latter interlocks with the per-session state refactor (2.4); the M14 test runs
   sequentially because `sim-devices` registries are process-global.

### 2.4 Per-session state ownership ("Move State to Per-World Ownership")  ⟶ BLOCKED: large, high-risk refactor needing sign-off

**What the plan wants:** move virtual devices, inspection state, virtual clock/task
identity, and network/display/touch state out of **process-global / thread-local** maps
that currently cross session boundaries — so concurrent in-process sessions don't share
`SIM_NOW`, `CURRENT_TASK_ID`, device registries, or network state. Keep only a narrow
active-simulator guard for FFI callbacks while guest code runs.

**Why it is blocked / not done autonomously:**
1. It is the **highest-risk** change in the whole plan. `SIM_NOW` and `CURRENT_TASK_ID` are
   process-global values read by the **C FFI** during guest firmware execution; moving them
   to per-World ownership touches the FFI callback boundary and the C scheduler's
   assumptions. A subtle mistake corrupts every firmware scenario's timing/trace.
2. It is large and staged (devices, then clock/task identity, then net/display/touch), with
   real chance of destabilizing the 29 golden-trace scenarios.
3. The current `simfarm` lane already documents this: because the microcar binary is
   **one World per process**, concurrency there is across processes — which still proves
   trace determinism under concurrent load and panic isolation. True *in-process*
   multi-session isolation is exactly this milestone.
4. It partly interlocks with the gRPC control-plane unification (2.3), which is now
   buildable locally via the workspace `protoc` path.

**To unblock:** explicit sign-off to take on the refactor, ideally with agreement to do it
in small verified stages (start with the device registry, keep golden traces byte-identical
at each step) — and awareness it may take several iterations.

### 2.5 `telematics` lane  ⟶ PARTIALLY BLOCKED: needs telematics firmware + host-socket rig

**What the plan wants:** start a `costar` server, run a telematics scenario, connect a host
TCP client, force small socket buffers, send bursts, and assert every request gets a
response with no frame drops and repeated wakeups working.

**Why it is blocked:** the host-networking engine edges are already hardened (M1:
`sim_net_drain_tx` conservation, host-poller re-arm, TCP partial-write framing). But the
lane needs (a) **telematics firmware** that performs periodic host uploads / remote queries
(firmware work, 2.1-adjacent), and (b) a host-side TCP test rig driving the TAP/socket path.
It is less blocked than cockpit (no `protoc`), but still gated on new firmware behavior.

**To unblock:** approve the firmware track; the host-socket harness itself is buildable in
the std-only dogfood crate once there is firmware that talks to the host.

### 2.6 Nightly / scale + remaining breakpoint predicates  ⟶ mostly firmware-gated

- Remaining breakpoint predicates (machine, vehicle-state, device-state, DTC-creation,
  assertion-failure) are thin wrappers over the existing `continue_until` mechanism.
  Vehicle state and DTC traces now exist for diagnostics, but device-state/assertion-failure
  predicates still need product events. The message breakpoint (`run_to_frame`) is done.

---

## 3. Summary of what input is needed

The autonomous track has cleared the bounded environment issue and delivered the charging
safety lane (M13), the cockpit gRPC-surface proof (M14), the OTA happy path (M15), the OTA
slot-metadata model + first rollback fault (M16), and three more OTA fault-matrix cases
(M17–M18, `harness ota` now 5/5 — 4 of 8 fault cases). The remaining work is now
**decision-gated**: the easily-reachable, no-modeling-decision-required increments are done,
and further progress needs the user to choose a modeling approach or grant sign-off:

1. **Remaining firmware EV lanes** — the charging *safety* lane (M13) and 4 OTA fault cases
   (M16–M18) are done. What remains needs approval + a confirmed modeling approach: the
   richer charging FSM (handshake / temperature-rise / reduced-current / charge-complete) and
   battery plant physics; the last OTA fault cases (**gateway reset** / **BMS critical fault**
   during update — need a reliable cross-ECU/reset mechanism, not a fabricated trace stub) and
   the mode-gated **OTA/charging-while-drive** cases (need a scenario-schema design for
   representing OTA/charging mode in TOML — shared with the two deferred `toml_zoo` cases);
   and diagnostics live-BMS. These also unblock the debug_gym seeded-bug corpus (an
   OTA-rollback seed is trivially seedable from the M16–M18 fault variants once the corpus
   format is agreed).
2. **Cockpit / gRPC lane** — no longer blocked by `protoc`; proceed with the local
   `PROTOC` path above.
3. **Per-session state refactor** — the other big costar track; high-risk, needs sign-off
   and a staged, heavily-verified approach.

Recommended order if all are eventually wanted: continue the **OTA fault matrix** (cheap,
reuses the M16 model) and the **deeper charging FSM** → **cockpit / gRPC** display firmware
→ **per-session state** last (riskiest, but needed for product-grade in-process
session/device isolation).
