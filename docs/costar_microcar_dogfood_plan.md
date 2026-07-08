# costar + microcar Compact-EV Dogfood Plan

## Summary

This plan turns `microcar` into the canonical dogfood project for `costar`: a compact passenger-EV embedded-system benchmark that exercises the simulator surfaces the product needs most.

The work should be sequenced in two tracks:

1. Stabilize `costar`'s simulator chassis: correctness, per-session isolation, trace identity, topology, breakpoints, replay, and control-plane reliability.
2. Evolve `microcar` from a small single-bus demo into a compact-EV dogfood suite with realistic vehicle lanes: concurrent fleet runs, malformed scenario robustness, multi-bus topology, cockpit HMI, debug gym, diagnostics, telematics, charging, and OTA.

Automotive-specific behavior belongs in `microcar`. Generic simulation infrastructure belongs in `costar`.

## Source Context

This plan is based on:

- `/Users/zmm/Documents/REPORT.html`
- `/Users/zmm/Documents/SUGGESTIONS.html`
- `/Users/zmm/Downloads/car_costar_dogfood_handoff.md`
- The current `costar` and `microcar` repository layouts and docs.

The reports agree on the core diagnosis: `costar` has the right engine, but the long-lived product chassis is not ready. The current `microcar` tests the engine shape but does not yet force enough of the product-facing surfaces: gRPC sessions, concurrent runs, device inspection, display/touch streaming, trace correlation, topology graphing, breakpoints, replay, and hostile input.

## Strategy

Use `microcar` as a progressively realistic compact passenger-EV benchmark. Do not pursue high-fidelity car physics. Each new vehicle subsystem should exist because it forces a missing `costar` capability.

Priority order:

1. Fix `costar` correctness and isolation bugs that would make dogfood results untrustworthy.
2. Add dogfood harness infrastructure in `microcar`.
3. Add the cheapest high-yield dogfood lanes first: `simfarm` and `toml_zoo`.
4. Add product-facing lanes: topology, cockpit HMI, debug gym, diagnostics, telematics, charging, and OTA.

## costar Roadmap

### Stabilize the Engine

- Fix `SimulatorCore::run_until` so cancelled tombstones cannot panic and live events past the deadline are not dispatched early.
- Fix active fiber yielder handling so the current fiber's `Yielder` is refreshed on every resume and cleared/restored when control returns to the scheduler.
- Add regression tests for cancel-then-`run_until`, deadline overshoot, clock rollback, and multi-fiber resume/suspend behavior.
- Add stepped-vs-continuous equivalence tests for deterministic replay confidence.

### Move State to Per-World Ownership

- Move virtual devices, inspection state, virtual clock/task identity, and network/display/touch state out of process-global or thread-local maps where those maps cross session boundaries.
- Keep only a narrow active-simulator guard for FFI callbacks while guest code is executing.
- Ensure gRPC `ConfigureBoard`, `Run`, `InspectDevices`, touch injection, and display streaming operate on the same session-owned state.
- Prevent concurrent sessions from sharing `SIM_NOW`, `CURRENT_TASK_ID`, device registries, or network state.

### Unify Control-Plane Semantics

- Introduce a shared session/world runner abstraction used by JSON-RPC and gRPC.
- Avoid holding the session map lock for a full simulation run.
- Return the world to its session, or mark the session failed, on every run exit path.
- Add panic boundaries around guest-reachable execution paths.
- Make session listing deterministic.
- Bound or clean up sessions, keyframes, and trace buffers.
- Preserve compatibility adapters for existing JSON-RPC/gRPC clients while new dogfood tests target the gRPC product path.

### Make Trace v2 the Product Data Model

Trace v2 should include enough identity and causality for GUI packet animation and AI debugging:

- `trace_id`
- `correlation_id`
- `parent_id`
- `virtual_time`
- `machine_id`
- `machine_name`
- `component_id`
- `component_type`
- `port_id`
- `event_type`
- `direction`
- `bus_or_link_id`
- `message_id`
- `payload_summary`
- `task_id`
- `rtos`
- `source`
- `destination`

Required behavior:

- Every transmit-to-receive path carries a correlation ID.
- Gateway forwarding preserves parent/child causality.
- Display/device state can be reconciled against trace state at the same virtual time.
- Old human/JSONL trace output can be generated from trace v2 for compatibility.

### Add Topology and Debugging Primitives

- Represent topology as nodes with typed ports and typed edges, not only flat source-target links.
- Support multi-interface machines and gateway forwarding metadata.
- Add `step_event`, `step_message`, `step_task`, and `continue_until(predicate)`.
- Add breakpoint predicates for message, machine, vehicle state, device state, dropped frame, DTC creation, and assertion failure.
- Add keyframe save/restore and deterministic replay.
- Prefer replay checkpoints over coroutine-stack snapshots unless full stack snapshots become unavoidable.

### Harden Networking and Device Edges

- Preserve all queued TX frames in `sim_net_drain_tx`.
- Re-arm host-poller fds after readiness events.
- Handle partial TCP writes without corrupting framed streams.
- Prevent panics across FFI boundaries.
- Guard zero-length SPI RX fault-injection paths.
- Add timeout-wrapped integration tests for repeated host I/O wakeups and frame conservation.

## microcar Roadmap

### Reframe the Project

`microcar` should present as:

> A compact passenger-EV embedded-system benchmark for costar. It simulates a simplified EV ECU network, safety behavior, diagnostics, cockpit devices, telematics, charging, OTA, and fault scenarios under deterministic virtual time.

Avoid describing it as a go-kart, toy car, RC car, or physics simulator.

The current ECUs remain the core:

- Gateway ECU: vehicle mode authority, heartbeat monitor, fault aggregator, bus bridge.
- Powertrain ECU: accelerator/brake processing, torque command, torque limits.
- BMS ECU: SOC, voltage, current, temperature, torque limits, pack faults.
- Dashboard ECU: speed, warnings, drive mode, user-facing state.

Vehicle modes:

- `OFF`
- `ACCESSORY`
- `READY`
- `DRIVE`
- `LIMITED_POWER`
- `FAULT`

Later modes:

- `CHARGING`
- `SERVICE`
- `OTA_UPDATE`
- `TRANSPORT_MODE`

### Dogfood Harness Foundation

Add a reusable harness under `dogfood/` that can run scenarios through `costar`, normalize trace hashes, evaluate invariants, enforce timeouts, and emit JSON summaries for CI.

Prefer invariants over only golden traces:

- Virtual time never moves backward.
- `run_until` never passes its deadline.
- Stepped execution equals continuous execution.
- Keyframe restore reproduces future trace.
- No dropped or duplicated frames unless a fault requests it.
- Gateway forwarding preserves correlation IDs.
- Vehicle safety rules hold.
- `InspectDevices` state matches trace-derived state.
- Concurrent run trace equals solo run trace.
- Server remains responsive after bad input.

## Dogfood Lanes

### 1. simfarm

Purpose: test concurrent deterministic sessions and long-lived server behavior.

Contract:

- Run a scenario once and record a solo trace hash.
- Run N sessions for the same scenario in one server process.
- Assert every concurrent trace hash equals the solo hash.
- Add churn mode: create, load, run, destroy, repeat many times, and assert RSS plateaus.
- Add panic-isolation mode: one intentionally bad session fails cleanly while healthy sessions finish unchanged.

Forces:

- Per-session world isolation.
- No process-global clocks or task IDs.
- Panic boundaries.
- Session lifecycle correctness.
- Leak resistance.
- Deterministic traces under load.

### 2. toml_zoo

Purpose: test scenario validation and AI-agent robustness.

Malformed cases:

- Duplicate machine ID.
- Missing gateway.
- Bad bus reference.
- Unknown ECU firmware.
- Invalid fault target.
- Negative or huge duration.
- Duplicate bus node.
- Drive mode without powertrain.
- Charging while in drive.
- OTA while in drive.

Contract:

- Return structured errors.
- Never panic.
- Do not poison the server.
- Do not partially corrupt a session.
- Sibling sessions continue.

### 3. topology

Purpose: force realistic vehicle-network topology.

Phase 1 topology:

```text
vcan_drive
  gateway
  powertrain
  bms

vcan_body
  gateway
  dashboard
  body_control

vcan_diag
  gateway
  diagnostics_tool
```

Scenarios:

- `dual_bus_gateway.toml`
- `drive_body_bus_bridge.toml`
- `diag_request_through_gateway.toml`
- `bus_isolation_fault.toml`
- `gateway_loop_prevention.toml`
- `fleet_16_nodes.toml`
- `fleet_64_nodes.toml`

Assertions:

- Expected receivers get each frame.
- Unexpected receivers do not get each frame.
- Gateway bridge emits exactly one forwarded frame.
- Forwarded frame preserves correlation ID.
- No duplicate injection into a controller.
- Trace includes source and destination component identity.

### 4. cockpit

Purpose: dogfood the GUI-facing display/touch/control-plane path.

Normal-car dashboard states:

- Boot screen.
- Ready screen.
- Drive screen.
- Limited-power warning.
- Fault screen.
- Charging screen later.
- OTA progress screen later.

Harness flow:

```text
CreateSession
ConfigureBoard(display + touch + timer + ADC)
Load vehicle scenario
Run(stream_display = true)
InjectTouch(screen button)
InspectDevices
Assert framebuffer hashes
Assert dashboard state matches trace state
```

Forces:

- Per-World device ownership.
- Display streaming.
- Touch injection.
- `InspectDevices` correctness.
- gRPC reliability.
- Trace-to-device consistency.

### 5. debug_gym

Purpose: make `microcar` an AI-debugging benchmark.

Seeded bug examples:

- Gateway race condition.
- Powertrain timeout/cancel bug.
- BMS stale sensor bug.
- Dashboard missed warning bug.
- Telematics partial-write bug.
- OTA rollback bug.
- Gateway bridge loop bug.

Each bug should include:

- Bug description.
- Expected symptom.
- Minimal failing scenario.
- Golden failing trace.
- Required debugging primitive.
- Fixed trace.

Harness assertions:

- Run-to-completion trace equals stepped trace.
- `run_until` never overshoots.
- Clock never moves backward.
- Keyframe restore reproduces the same future.
- Breakpoint hits expected event.
- Correlation ID links cause to effect.

### 6. diagnostics

Purpose: test service-mode workflows and query/response traces.

Behavior:

- Diagnostic session starts.
- Tool reads vehicle mode.
- Tool reads DTCs.
- Tool clears DTCs.
- Tool requests live BMS data.
- Tool runs actuator self-test.
- Service mode disables drive.

Scenarios:

- `read_dtcs_after_bms_fault.toml`
- `clear_dtcs_after_fault_recovery.toml`
- `service_mode_disables_drive.toml`
- `live_data_stream.toml`
- `actuator_test_rejected_in_drive.toml`

### 7. telematics

Purpose: exercise host networking and long-running server behavior.

Behavior:

- Periodic status upload.
- Remote diagnostic query.
- Remote lock/unlock mock.
- Remote precondition request.
- Fault upload.

Harness:

- Start `costar` server.
- Run telematics scenario.
- Connect host TCP client.
- Force small socket buffers.
- Send multiple requests.
- Assert every request gets a response.
- Assert repeated wakeups work.
- Assert no frame drops under bursts.

### 8. charging

Purpose: add EV-specific state without heavy physics.

Behavior:

- Plug inserted.
- Handshake.
- Charging active.
- BMS temperature rises.
- BMS requests reduced current.
- Charge complete.
- Drive blocked while plugged.
- Fault stops charging.

### 9. ota

Purpose: test storage, reboot, rollback, and fault injection.

Behavior:

- Download image.
- Write slot B.
- Verify CRC.
- Commit boot flag.
- Reboot.
- Health check.
- Roll back on failed health.

Fault matrix:

- Power cut before write.
- Power cut during write.
- Power cut after write before commit.
- Corrupt image.
- Gateway reset during update.
- BMS critical fault during update.
- OTA requested while driving.
- OTA requested while charging.

## CI Strategy

### PR-fast

- `costar` build/test/clippy.
- Critical regression tests.
- Core `microcar` state tests.
- Short normal-drive scenario through dogfood harness.
- `toml_zoo` smoke.
- `simfarm N=2` on one short scenario.
- Determinism hash check for 2-3 scenarios.

### Medium / Main

- All current vehicle scenarios.
- All malformed scenarios.
- `simfarm N=4`.
- Cockpit framebuffer smoke.
- Multi-bus topology smoke.
- Diagnostics smoke.

### Nightly

- `simfarm N=8` and `N=16`.
- Churn/RSS slope.
- Long drive and soak scenarios.
- 64-node topology.
- Telematics socket-pressure tests.
- Debug gym seeded bugs.
- OTA fault-matrix subset.
- Sanitizer builds where practical.

### Manual / Demo

- Cockpit live display stream.
- AI-debugging gym walkthrough.
- OTA rollback demo.
- Diagnostics session demo.
- Telematics remote-command demo.

## Milestone 1 status (this branch)

The first implementation milestone — costar stabilization + microcar harness
foundation — is implemented on this branch:

- costar: `run_until` tombstone/overshoot fix (+ stepped-vs-continuous tests);
  fiber yielder refresh/clear (+ multi-fiber tests); `sim_net_drain_tx` frame
  conservation (`pop_tx`); host-poller re-arm; TCP partial-write framing
  (`out_buf`/`out_pos`); zero-length SPI RX fault guard; `RtosBackend`
  `zephyr` serde rename; FFI panic-boundary test.
- microcar: reframed docs (README + vehicle model); `dogfood/` harness crate
  (subprocess runner, FNV-1a trace hashing, invariant framework, wall-clock
  timeout, JSON summary, solo-vs-repeat determinism, `harness` CLI).

The later lanes (simfarm, toml_zoo, topology, cockpit, debug_gym, diagnostics,
telematics, charging, OTA) and the larger costar tracks (per-session state,
Trace v2, control-plane unification) remain for subsequent milestones.

## Milestone 2 status (this branch)

The second milestone — the two cheapest high-yield dogfood lanes (`simfarm`,
`toml_zoo`) plus the hostile-input handling they require — is implemented on
this branch:

- costar: Linux build fix so the workspace compiles off macOS
  (`sim-net` TAP fd non-blocking via `fcntl(F_SETFL)` with the Linux
  `O_NONBLOCK`; the old `File::set_nonblocking` is not a stable `std::fs::File`
  method and never compiled on Linux). Host-TAP path only — no effect on
  deterministic simulation.
- microcar binary: never panics on malformed input. `src/main.rs` handles every
  load/validate/build/run failure and prints a structured
  `microcar: error [<kind>]: ...` line on stderr with stable exit codes
  (`0` = pass, `1` = runtime fail, `2` = scenario error). New `src/validate.rs`
  adds the automotive-semantic checks — `unknown-firmware`, `missing-gateway`,
  `duplicate-bus-node`, `drive-without-powertrain` — while costar's
  `Scenario::from_file` already covers the structural checks (duplicate IDs,
  bad bus/link/fault references, TOML parse/range).
- `toml_zoo` lane: `dogfood/toml_zoo/` corpus of 11 malformed scenarios (each
  tagged `# expect-kind:`), `dogfood/src/toml_zoo.rs`, and `harness toml-zoo`.
  Asserts every case returns the expected structured error kind with exit 2 and
  no panic, plus sibling isolation (a malformed scenario run concurrently with a
  healthy one does not disturb it). The `charging-while-drive` / `ota-while-drive`
  cases are deferred to the charging/OTA lanes — they need vehicle modes not yet
  in the scenario schema.
- `simfarm` lane: `dogfood/src/simfarm.rs` and `harness simfarm`. Concurrent
  determinism (N sessions produce the same normalized trace hash as a solo run),
  churn (repeated create/run/destroy stays stable — no cross-launch state leak),
  and panic isolation (a malformed sibling fails cleanly while the healthy run is
  unaffected). Because the microcar binary hosts one `World` per process,
  concurrency here is across processes; true in-process multi-session isolation
  (shared `SIM_NOW`/`CURRENT_TASK_ID`/device registries) is a later costar-server
  milestone (the "Move State to Per-World Ownership" track).

Both lanes emit JSON summaries for CI and exit non-zero on failure. The dogfood
crate has 45 unit tests (determinism/invariants/trace-hash/summary + 8 simfarm +
8 toml_zoo) and is clippy-clean. The remaining lanes (topology, cockpit,
debug_gym, diagnostics, telematics, charging, OTA) and the larger costar tracks
(per-session state, Trace v2, control-plane unification) remain for subsequent
milestones.

## Milestone 3 status (this branch)

The third milestone — the `topology` lane plus the costar CAN bus-isolation fix
it rests on — is implemented on this branch:

- costar: **CAN TX bus isolation** (`sim-world/src/world.rs`). A firmware CAN
  send was previously placed onto *every* World bus regardless of which bus the
  sending machine was attached to, so multi-bus topology had no isolation. Now a
  send is only placed on the buses the machine is actually a node of (a
  multi-interface machine such as a gateway still sends on each of its buses).
  Covered by the `test_firmware_can_tx_respects_bus_membership` sim-world engine
  test. No existing scenario regresses: single-bus scenarios are unaffected, and
  the multi-bus fleet scenarios carry no golden trace.
- microcar: **topology lane** — `dogfood/topology/` scenarios
  (`dual_bus_gateway`, `diag_request_through_gateway`, `fleet_16_nodes`),
  `dogfood/src/topology.rs`, and `harness topology`. Each scenario declares
  `# topology-probe: probe=0xNNNN expect=<machine ids>` directives; the harness
  injects unique-id CAN probes (0x07xx, unused by the ECU protocol) on each bus
  and asserts, from the `can-rx` trace, that every probe reaches exactly the
  declared receivers — once each — and no node on any other bus. That covers the
  plan's "expected receivers get each frame", "unexpected receivers do not", and
  "no duplicate injection into a controller" assertions.

Deferred to the Trace-v2 / gateway-bridge milestone (they need correlation ids,
per-event source/destination identity, and gateway frame *forwarding*, none of
which exist yet): the `drive_body_bus_bridge`, `gateway_loop_prevention`, and
`fleet_64_nodes` scenarios, and the "forwarded frame preserves correlation id" /
"trace includes source and destination component identity" assertions. The
dogfood crate now has 52 unit tests (+7 topology) and stays clippy-clean.

## Milestone 4 status (this branch)

The fourth milestone — the **Trace v2 foundation**, gated opt-in so existing
golden traces stay byte-identical — is implemented on this branch:

- costar: `sim_core::TraceV2` (`trace_id`, `correlation_id`, `virtual_time`,
  `event_type`, `direction`, `bus_or_link_id`, `message_id`, `source`,
  `destination`, `len`), `serde::Serialize` with `to_json_line()` (JSONL) and
  `to_human_line()` — the latter regenerates the legacy `can-rx`/`can-tx` text,
  satisfying the plan's "old human/JSONL trace output can be generated from
  trace v2". `World` gains an opt-in v2 sink (`enable_trace_v2` /
  `drain_trace_v2` / `trace_v2_jsonl`). When enabled, `deliver_buses` emits, per
  CAN send, one `tx` edge plus one `rx` edge per receiver, all sharing a
  correlation id derived from the per-send bus sequence (`CanBus::drain_arrived`
  now surfaces `seq`). Default off ⇒ the human/golden trace is byte-identical
  (verified: a scenario's stdout is identical with and without the flag).
  Covered by `test_trace_v2_correlation_and_identity` and
  `test_trace_v2_disabled_by_default`; sim-world now has 104 tests.
- microcar: `microcar <scenario> --trace-v2 <path>` enables the sink and writes
  the v2 records as JSONL after the run. It never changes the default stdout
  output or the exit codes (0/1/2); the JSONL write is best-effort.

This delivers the plan's "every transmit-to-receive path carries a correlation
id" and "trace includes source and destination component identity". The
topology lane now **consumes** the v2 JSONL: `harness topology` runs each
scenario with `--trace-v2` and, per probe, asserts every delivery shares one
correlation id and one source and that the destinations equal the expected
receivers (the plan's "forwarded frame preserves correlation id" for direct
sends). The dogfood crate now has 56 unit tests (+4 trace-v2). Remaining Trace
v2 fields (`parent_id`, component/port ids, `task_id`, `rtos`,
`payload_summary`) and gateway parent/child *forwarding* causality (which the
`drive_body_bus_bridge` / `gateway_loop_prevention` scenarios need) are additive
follow-ups.

## Milestone 5 status (this branch)

The fifth milestone — **gateway bus forwarding** with parent/child causality,
completing the topology lane's bridge scenario — is implemented on this branch:

- costar: opt-in multi-interface **bridging**. A machine declared a bridge
  (`[[bridge]] machine = "…"`) forwards a frame it receives on one bus onto its
  other buses, exactly once (hop-based loop prevention). Gated behind the
  declaration, so scenarios without a `[[bridge]]` are byte-identical (zero
  regression). `CanBus` carries `hop` + `parent_correlation` and gains
  `forward()`; `World::deliver_buses` collects forward actions for bridge
  receivers of original (hop-0) frames and re-transmits them after the drain
  pass. `sim_core::TraceV2` gains `parent_id`: a forwarded frame's records carry
  the correlation id of the frame that caused the forward. Correlation ids are
  1-based (0 = "no parent"). Covered by
  `test_gateway_forwarding_parent_causality`; sim-world has 105 tests.
- microcar: `dogfood/topology/drive_body_bus_bridge.toml` (the gateway bridges
  `vcan_drive` ↔ `vcan_body`); the topology harness is now forwarding-aware — it
  parses `parent_id` from the v2 JSONL and, per probe, accepts exactly one root
  correlation plus forwarded edges whose `parent_id` links back to it, with the
  full receiver set delivered once each.

This realizes the plan's topology assertions "gateway bridge emits exactly one
forwarded frame", "forwarded frame preserves correlation id", and "trace
includes source and destination component identity". The topology lane now
covers 5 scenarios (`dual_bus_gateway`, `diag_request_through_gateway`,
`fleet_16_nodes`, `drive_body_bus_bridge`, `gateway_loop_prevention`), all green.
`gateway_loop_prevention` demonstrates single-hop forwarding — a forwarded frame
is never re-forwarded, so it cannot chain or loop across a bridge chain; the
complementary multi-bridge de-duplication (several bridge paths inject a
controller on a shared bus only once, keyed on `(source_bus, seq, target_bus)`)
is covered by the costar `test_gateway_forwarding_dedups_multiple_bridges` engine
test (sim-world now has 106 tests). Only `fleet_64_nodes` (a nightly-scale
variant of `fleet_16_nodes`) remains deferred.

## Milestone 6 status (this branch)

The sixth milestone completes the **Trace v2 product data model** field set. In
addition to the identity/causality fields from M4–M5 (`trace_id`,
`correlation_id`, `parent_id`, `virtual_time`, `event_type`, `direction`,
`bus_or_link_id`, `message_id`, `source`, `destination`, `len`),
`sim_core::TraceV2` now carries `machine_id`, `machine_name`, `component_id`,
`component_type`, `port_id`, `payload_summary`, `task_id`, and `rtos` — the full
list from the plan's "Make Trace v2 the Product Data Model".

For CAN delivery edges these are populated as: `machine_id`/`machine_name` = the
primary machine (receiver for `rx`, sender for `tx`, name resolved from the
World); `component_id` = 0 / `component_type` = `"can_controller"`;
`payload_summary` = a compact lowercase-hex of the first 8 payload bytes (handy
for GUI packet animation / AI debugging). `port_id`, `task_id`, and `rtos` are
reserved (empty / 0) for the typed-port-topology and task/device event types
that will populate them. Still opt-in: a scenario's default stdout is
byte-identical with and without `--trace-v2` (re-verified). sim-world has 106
tests; the topology lane's JSONL parser is unaffected by the added fields.

## Milestone 7 status (this branch)

The seventh milestone adds the first **debugging primitives** from the plan's
"Add Topology and Debugging Primitives" costar section:

- `World::step()` advances the simulation by exactly one virtual-time event,
  returning `StepOutcome::Advanced(now)` or `StepOutcome::Done`. `run()` and
  `run_until()` are refactored to `while running { step()? }`, so a stepped
  replay is **trace-identical to a continuous run by construction** — the
  debug_gym "run-to-completion trace equals stepped trace" invariant. Verified
  by `test_stepped_equals_continuous` and by re-running all 29 non-soak
  scenarios (golden traces unchanged).
- `World::continue_until(predicate, deadline)` steps until a `FnMut(&World)`
  predicate holds (returning whether it matched, or `false` at the deadline / on
  completion) — the `continue_until(predicate)` primitive and the basis for
  breakpoints. Verified by `test_continue_until_stops_at_predicate`.

These build directly on the existing deterministic run loop with no new firmware
or external dependencies. sim-world now has 108 tests; the run-loop refactor is
byte-identical for every existing scenario (golden traces intact) and
clippy/fmt-clean. The remaining debug_gym pieces (keyframe-restore replay
wiring, the seeded-bug corpus with golden failing/fixed traces, and a microcar
`--step` harness mode) build on these primitives.

## Milestone 8 status (this branch)

The eighth milestone wires the M7 debugging primitives into an observable
**debug_gym** dogfood lane (no new firmware or external deps):

- microcar: `microcar <scenario> --step` drives the run one event at a time via
  `World::step()`; its output is byte-identical to a continuous run (verified).
  `sim_world` re-exports `StepOutcome`.
- microcar dogfood: `dogfood/src/debug_gym.rs` + `harness debug-gym`. Per
  scenario it runs continuous and `--step` and asserts, end-to-end through the
  product binary: **run-to-completion trace equals stepped trace** (trace-hash
  compare), **`run_until` never overshoots** (max trace virtual time ≤ the
  `duration_ms` deadline), and **clock never moves backward** (the segment-aware
  monotonic check). `run_scenario_args` was added to the harness runner to pass
  extra CLI flags such as `--step`.

`harness debug-gym` runs a built-in set of short vehicle scenarios by default
(override with `--scenario-dir`) and is 4/4 green. The dogfood crate has 59 unit
tests, is clippy/fmt-clean, and the other lanes (topology 5/5, toml_zoo 11/11,
simfarm) are unregressed. The remaining debug_gym pieces — the seeded-bug corpus
(golden failing/fixed traces per bug) and keyframe-restore replay (on the
existing `save_keyframe`/`load_keyframe` scaffolding) — build on this.

## Milestone 9 status (this branch)

The ninth milestone completes the **topology** lane — all seven plan scenarios
now pass (`harness topology` → 7/7). microcar-only (two new scenario files, no
costar change, no regression to the other lanes):

- `fleet_64_nodes.toml`: bus isolation at scale — 64 machines across 8 CAN
  buses, the gateway the only shared hub node; three probes (8- and 7-node
  fanout) each reach exactly their own bus and no node on any other, with v2
  correlation/identity intact.
- `bus_isolation_fault.toml`: a `drop_frame` fault isolates one bus. The gateway
  bridges `vcan_a` ↔ `vcan_b`; a fault drops the probe id on `vcan_b` (applied
  before the probe via an early warm-up tick), so the gateway's *forwarded* copy
  is dropped and the `vcan_b` node is isolated while the `vcan_a` receivers
  still get it. This combines M5 gateway forwarding with the existing
  `drop_frame` bus fault: the forwarded frame is enqueued *during* the run, so
  (unlike a pre-queued `bus_inject`) a mid-run drop fault can actually affect it
  — which is exactly why this scenario was deferred until forwarding existed.

The topology lane now covers every scenario listed in the plan
(`dual_bus_gateway`, `drive_body_bus_bridge`, `diag_request_through_gateway`,
`bus_isolation_fault`, `gateway_loop_prevention`, `fleet_16_nodes`,
`fleet_64_nodes`), all green.

## Milestone 10 status (this branch)

The tenth milestone adds **keyframe save/restore + deterministic replay** — the
costar "keyframe save/restore and deterministic replay" primitive and the
debug_gym "keyframe restore reproduces the same future" invariant:

- `World::replay_from_keyframe(kf)` reconstructs the World at a keyframe by a
  **replay checkpoint** (the plan's explicitly preferred approach over
  coroutine-stack snapshots): it rebuilds a fresh World from the keyframe's
  stored `scenario_toml` and deterministically runs forward to `kf.now`. Because
  the engine is deterministic, replaying to the checkpoint reproduces the exact
  state, and continuing reproduces the exact future.
- Covered by `test_keyframe_replay_reproduces_future`: a single continuous run's
  trace is byte-identical to (replay to a mid-run keyframe) + (continue to
  completion) from a freshly-rebuilt World. sim-world now has 109 tests;
  clippy/fmt-clean; `run`/`run_until` unchanged (no scenario regression).

Firmware-driven replay (reconstructing guest firmware state, not just bus/link
delivery) remains a later milestone — costar's `build_world` produces bare
machines, so this replay path is exact today for scenarios whose observable
trace is driven by bus/link delivery (e.g. `bus_inject`).

## Milestone 11 status (this branch)

The eleventh milestone adds a **message breakpoint** on top of `continue_until`
(the plan's "breakpoint predicates for message …"):

- `World::run_to_frame(frame_id, deadline)` runs until a CAN frame with
  `frame_id` is delivered (a `can-rx` for it appears in any machine's trace),
  returning whether the breakpoint was hit. Covered by
  `test_run_to_frame_breakpoint`.
- Fixed a correctness bug in `continue_until` that this test surfaced: when the
  matching event was delivered in the same step that drained the last pending
  event (so the World went idle and `step()` returned `StepOutcome::Done`), the
  loop broke *without* evaluating the predicate, missing a breakpoint on the
  final event. The predicate is now checked on the `Done` step too.

sim-world has 110 tests; clippy/fmt-clean; `run`/`run_until` unchanged (no
scenario regression). Remaining breakpoint predicates (machine, vehicle state,
device state, DTC creation, assertion failure) are thin wrappers on the same
`continue_until` mechanism but mostly become meaningful once firmware emits
those events.

## Milestone 12 status (this branch)

The twelfth milestone added the first **diagnostics** dogfood lane
(recorded in detail in `docs/BLOCKERS.md`): protocol support for `SERVICE`,
`OTA_UPDATE`, `TRANSPORT_MODE`, diagnostic request/response IDs
(`0x600`/`0x601`), gateway DTC/session handling, a diagnostics-tool ECU,
service-mode torque blocking, and a `harness diagnostics` lane. Like the later
EV lanes it is **trace-backed** — it asserts compact firmware `user-u32` trace
events emitted by explicit dogfood firmware variants (`gateway_diag`,
`gateway_diag_fault`, `powertrain_diag_service`) rather than depending on the
still-unreliable firmware-originated CAN RX/TX path. `harness diagnostics` is
2/2 green (`read_clear_dtcs_after_bms_fault`, `service_mode_disables_drive`),
covering SERVICE-mode entry, read-mode, read/clear DTCs, actuator self-test, and
the SERVICE torque clamp.

## Milestone 13 status (this branch)

The thirteenth milestone adds the first **charging** dogfood lane — the plan's
`charging` EV lane, "drive blocked while plugged". It is **microcar-only** (no
costar change), gated behind opt-in dogfood firmware variants so every existing
golden trace stays byte-identical (verified: the `debug_gym` trace hashes for
`bms_overtemp_limp_mode` and `brake_overrides_throttle` are unchanged, and all
29 non-soak vehicle scenarios still pass).

Key realization: the vehicle-mode state machine and the safety clamp **already**
model charging. `mc_gateway_determine_mode` keeps `VEHICLE_CHARGING` sticky
(only a critical fault overrides it), and `mc_safety_clamp_torque` /
`safety_mode_blocks_torque` already zero positive torque and disable the motor
outside `DRIVE`/`LIMP`. What was missing was (a) any way to *enter* CHARGING
(no plug trigger) and (b) a lane to exercise the safety property end-to-end.

Following the M12 diagnostics dogfood-script pattern (trace-backed, no reliance
on firmware CAN RX):

- firmware: `gateway_enable_dogfood_charging_script()` + `run_dogfood_charging_script()`
  in the gateway ECU — at 100 ms a charger is "plugged in" (gateway enters
  CHARGING, traces `gateway_charging_state`); at 300 ms a drive request arrives
  while plugged and `gateway_state_enter_drive` is a no-op outside READY, so the
  vehicle stays in CHARGING (traces `charging_drive_blocked=1`).
  `powertrain_enable_dogfood_charging()` forces a CHARGING-mode torque
  computation with an 80% throttle demand and traces `charging_motor_command`
  (torque + motor_enable), so the clamp is exercised. Two new boot functions
  (`microcar_boot_gateway_charging`, `microcar_boot_powertrain_charging`) wire
  the flags; `src/lib.rs` resolves `firmware/gateway_charging_ecu` /
  `firmware/powertrain_charging_ecu`. All charging behavior is behind flags that
  default to `0`, so the default gateway/powertrain firmware is unchanged.
- microcar dogfood: `dogfood/charging/plug_blocks_drive.toml` (with
  `# charging-expect:` directives), `dogfood/src/charging.rs`, and
  `harness charging`. The lane asserts `charging-mode` (gateway entered
  CHARGING), `drive-blocked` (a drive request while plugged left the vehicle in
  CHARGING), and `motor-torque max=0` (every CHARGING motor command clamped
  torque ≤ 0 with the motor disabled). `harness charging` is 1/1 green.

The dogfood crate now has 68 unit tests (+6 charging); state_tests 92; all other
lanes unregressed (`topology` 7/7, `toml_zoo` 11/11, `simfarm` PASS,
`debug-gym` 4/4, `diagnostics` 2/2); clippy/fmt-clean. The deferred
`toml_zoo` `charging-while-drive` case still needs charging represented in the
scenario *schema* (a plug input / mode field), which this firmware-side lane
does not yet add; OTA and the charging plant physics (temperature-rise /
reduced-current / charge-complete) remain follow-ups.

## Milestone 14 status (this branch)

The fourteenth milestone delivers the first **cockpit** lane increment — proving
the `sim-grpc` GUI-facing gRPC control plane end-to-end (UNBLOCKING.md §5
Strategy A "prove the existing gRPC surface" + Strategy C interaction/inspection).
It is **costar-only** and **additive test-only**: a new integration test at
`crates/sim-grpc/tests/cockpit_test.rs` (376 lines, one `#[tokio::test]`
`cockpit_grpc_surface_and_determinism`) with no change to server/session logic,
so golden traces are unaffected.

The old `protoc` blocker (§2.3) is cleared via the workspace compiler; the lane
is built/tested with `PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc
cargo test -p sim-grpc`.

The test exercises the full cockpit flow against the real gRPC product surface:
`CreateSession → LoadScenario → ConfigureBoard(display + touch + timer + ADC ⇒
n_peripherals=4) → Run(stream_display=true, stream_trace=true)` with an injected
touch press + release then `Stop`, collecting the `RunEvent` stream (Tick /
Trace / Display / End, asserting a `SimulationEnd` with no `SimulationError`),
then `InspectDevices` reconciliation against `ConfigureBoard` (the display's
width/height/color_mode, plus touch/timer/adc presence). Determinism is asserted
by running the **entire flow twice, sequentially**, and comparing an aggregated
`CockpitRunResult`: the FNV-1a framebuffer-byte hash, the DisplayFrame count, the
tick-boundary timestamp sequence, and the `SimulationEnd` totals must all be
identical.

**Honesty note (no fabricated pixels):** none of these scenarios contain firmware
that draws to the display, so the Run stream emits **zero DisplayFrame events**
and the framebuffer-byte set is empty — the determinism check is `empty == empty`
(a valid determinism assertion) alongside the tick/end-totals determinism. Rich
framebuffer-content assertions (Strategy B) depend on display-driving dashboard
firmware and are a deliberate follow-up. The two runs are **sequential** (not
concurrent) because the `sim-devices` device registries are process-global;
true in-process multi-session isolation is the deferred "Move State to Per-World
Ownership" track (BLOCKERS §2.4).

Verified locally: `PROTOC=… cargo test -p sim-grpc` → the 14 existing sim-grpc
integration tests **plus** the new cockpit test all pass (15 total). Remaining
cockpit follow-ups: a microcar `harness cockpit` wrapper (deferred to keep the
dogfood harness std-only — it would need a gRPC client or a shell-out),
display-driving dashboard firmware for real framebuffer-hash content, and the
dashboard-state↔trace reconciliation.

## Milestone 15 status (this branch)

The fifteenth milestone adds the first **ota** lane increment — the OTA
happy-path (UNBLOCKING.md §2 Strategy A "happy path first"), following the exact
M12 diagnostics / M13 charging trace-backed dogfood-script pattern. It is
**microcar-only** and gated behind an opt-in flag that defaults off, so every
existing golden trace stays **byte-identical** (verified: the `debug_gym` trace
hashes `c371b96e253e` / `68806f23c980` / `16684074b01c` / `fa7f03709681` for the
four scenarios are unchanged).

- firmware: `gateway_enable_dogfood_ota_script()` + `run_dogfood_ota_script()` in
  the gateway ECU, gated by `g_ota_dogfood_script` (default `0`). Called from the
  gateway main task loop next to the diag/charging scripts, it steps the minimal
  OTA state machine and traces compact `user-u32` events: `ota_state`
  `IDLE(0)→DOWNLOADING(1)→VERIFYING(2)→COMMIT_PENDING(3)→REBOOTING(4)→HEALTHY(5)`
  at 100–600 ms, plus `ota_crc_ok=1` (VERIFYING), `ota_slot=1` (COMMIT_PENDING,
  slot B), and `ota_boot_result=1` (HEALTHY). `microcar_boot_gateway_ota()`
  (in `microcar_coordinator.c`) enables the flag then boots the gateway;
  `src/lib.rs` resolves `firmware/gateway_ota` (ordered before the generic
  `gateway`). Because the flag defaults to `0`, the default gateway firmware is
  unchanged.
- microcar dogfood: `dogfood/ota/happy_path.toml` (with `# ota-expect:
  state-sequence 0,1,2,3,4,5`, `crc-ok`, `healthy`), `dogfood/src/ota.rs`
  (mirrors `charging.rs`: parses the directives into a `StateSequence`/`CrcOk`/
  `Healthy` expectation set, decodes the `user-u32` traces, asserts the expected
  monotonic `ota_state` subsequence occurred, `ota_crc_ok=1`, and
  `ota_boot_result=1`, with a JSON report), and a `harness ota` subcommand.

Verified locally: `cargo build --bin microcar` OK; the dogfood crate now has
**75 unit tests** (68 → 75, +7 OTA), all passing; `harness ota` is **1/1** green
(state-sequence, crc-ok, healthy); and every other lane is unregressed
(`charging` 1/1, `diagnostics` 2/2, `debug-gym` 4/4 with unchanged hashes,
`topology` 7/7, `toml-zoo` 11/11). Remaining OTA follow-ups: the 8-case
power-cut/corruption/reset **fault matrix** and rollback (UNBLOCKING §2
Strategy C), a pure slot-metadata model with commit/rollback unit tests
(Strategy B), the OTA-rollback `debug_gym` seed, and flipping the deferred
`toml_zoo` `ota-while-drive` case (needs OTA represented in the scenario
*schema*).

## Milestone 16 status (this branch)

The sixteenth milestone starts the **OTA fault matrix** with a pure
**slot-metadata model + rollback** (UNBLOCKING.md §2 Strategy B "storage/slot
model first" + the first Strategy C fault case). It is **microcar-only** and
gated behind opt-in dogfood firmware, so every existing golden trace stays
**byte-identical** (verified: the `debug_gym` trace hashes `c371b96e253e` /
`68806f23c980` / `16684074b01c` / `fa7f03709681` are unchanged, and the OTA
happy path emits the exact same `ota_*` events as M15).

- **Slot-metadata model (canonical C + Rust mirror).**
  `common/src/microcar_ota_slot.c` + `common/include/microcar_ota_slot.h`: a
  pure, deterministic A/B-slot state machine — `active_slot`, `target_slot`,
  `crc_ok`, `image_written`, `committed`, `boot_healthy`, `rolled_back` — with
  `mc_ota_{init,begin_download,finish_download,verify,commit,reboot,
  health_check,rollback,boot_slot}`. The rules never arm a bad slot: a failed
  CRC (corrupt image), an interrupted write, or a failed post-reboot self-test
  all roll back to the previous known-good slot, and a committed HEALTHY update
  is never undone. Registered in `build.rs`. `state_tests/src/ota_slot.rs`
  mirrors it in pure Rust with **7 unit tests** covering the Strategy B success
  criteria — commit, corrupt-CRC rollback, failed-health rollback,
  interrupted-write rollback, plus healthy-is-permanent, commit-requires-written,
  and state-guarded transitions (`state_tests` 92 → **99**).
- **Firmware drives the model.** `run_dogfood_ota_script` in the gateway ECU now
  steps the C model through the campaign and traces each transition, so the lane
  asserts the model's real behavior end-to-end. The happy path
  (`g_ota_fault_mode == OTA_FAULT_NONE`) is byte-identical to M15. A new
  `gateway_ota_badcrc` variant (`gateway_enable_dogfood_ota_fault_bad_crc()`,
  `OTA_FAULT_BAD_CRC`) injects a corrupt image: verification fails, so the model
  refuses to commit and rolls back —
  `IDLE(0) → DOWNLOADING(1) → VERIFYING(2, crc BAD) → ROLLED_BACK(6, slot A)`,
  emitting `ota_crc_ok=0`, `ota_rollback=1`, `ota_active_slot=0`,
  `ota_boot_result=0`. Wired via `microcar_boot_gateway_ota_badcrc()`
  (coordinator) and the `src/lib.rs` resolver (the more-specific
  `gateway_ota_badcrc` key is matched **before** `gateway_ota` in both
  `ecu_type()` and `init()`).
- **Harness.** `dogfood/src/ota.rs` gains three expectations — `crc-bad`,
  `rolled-back`, and `active-slot N` (plus a `last_value` helper) — and 5 new
  unit tests (dogfood 75 → **80**). New scenario
  `dogfood/ota/rollback_bad_crc.toml` asserts the rollback state sequence
  `0,1,2,6`, a bad CRC (and never a good one), a rollback occurred, and the
  bootloader active slot stayed at slot A (0).

Verified locally: `cargo build --bin microcar` OK; `state_tests` **99** and the
dogfood crate **80** unit tests pass; `harness ota` is **2/2** green
(`happy_path` + `rollback_bad_crc`); every other lane is unregressed
(`charging` 1/1, `diagnostics` 2/2, `debug-gym` 4/4 with unchanged hashes,
`topology` 7/7, `toml-zoo` 11/11); and the new Rust files are clippy-clean.
Remaining OTA follow-ups: the rest of the 8-case power-cut/corruption/reset
**fault matrix** (interrupted write, power-cut-before-commit, failed boot →
rollback, gateway/BMS reset during update) reusing this model, the OTA-rollback
`debug_gym` seed, and flipping the deferred `toml_zoo` `ota-while-drive` case
(needs OTA in the scenario *schema*).

## Milestone 17 status (this branch)

The seventeenth milestone extends the **OTA fault matrix** with two more cases,
both reusing the M16 slot-metadata model (UNBLOCKING.md §2 Strategy C,
"fault-matrix driven" — each case is a firmware fault variant + scenario wired
exactly like `gateway_ota_badcrc`). It is **microcar-only** and gated behind
opt-in dogfood firmware, so every existing golden trace stays **byte-identical**
(verified: the `debug_gym` trace hashes `c371b96e253e` / `68806f23c980` /
`16684074b01c` / `fa7f03709681` are unchanged, and the OTA happy path + bad-CRC
rollback emit the exact same `ota_*` events as M15/M16).

- **Firmware.** `run_dogfood_ota_script` is refactored around a shared
  `emit_ota_rollback()` helper so every fault aborts with an identical marker set
  (`ota_state=6`, `ota_rollback=1`, `ota_active_slot`, `ota_boot_result=0`),
  rolling back at whichever step its fault strikes. Two new fault selectors +
  boot variants:
  - `OTA_FAULT_INTERRUPTED_WRITE` (`gateway_ota_intwrite`): a power cut during the
    image write — `mc_ota_finish_download(complete=0)` discards the partial image
    and rolls back *before verifying*: `IDLE(0) → DOWNLOADING(1) → ROLLED_BACK(6,
    slot A)`.
  - `OTA_FAULT_BAD_HEALTH` (`gateway_ota_badhealth`): a valid image downloads,
    verifies (crc ok) and commits, but the post-reboot self-test fails —
    `mc_ota_health_check(healthy=0)` reverts to slot A:
    `IDLE(0) → DOWNLOADING(1) → VERIFYING(2, crc ok) → COMMIT_PENDING(3, slot B)
    → REBOOTING(4) → ROLLED_BACK(6, slot A)`.
  Wired via `microcar_boot_gateway_ota_intwrite()` /
  `microcar_boot_gateway_ota_badhealth()` (coordinator) and the `src/lib.rs`
  resolver (both new keys matched **before** `gateway_ota`). The happy path and
  the M16 bad-CRC path are byte-for-byte unchanged (the download-complete and
  boot-healthy selectors default true for those modes).
- **Harness / scenarios.** No new expectation types were needed — the M16
  `state-sequence` / `crc-ok` / `crc-bad` / `rolled-back` / `active-slot`
  expectations already express both cases. New scenarios
  `dogfood/ota/rollback_interrupted_write.toml` (asserts `0,1,6` + rolled-back +
  active-slot 0) and `dogfood/ota/rollback_failed_health.toml` (asserts
  `0,1,2,3,4,6` + crc-ok + rolled-back + active-slot 0). Added 2 dogfood unit
  tests over synthetic traces (dogfood 80 → **82**).

Verified locally: `cargo build --bin microcar` OK; `state_tests` **99** and the
dogfood crate **82** unit tests pass; `harness ota` is **4/4** green
(`happy_path`, `rollback_bad_crc`, `rollback_failed_health`,
`rollback_interrupted_write`); every other lane is unregressed (`charging` 1/1,
`diagnostics` 2/2, `debug-gym` 4/4 with unchanged hashes, `topology` 7/7,
`toml-zoo` 11/11); and the touched Rust file is clippy-clean. The OTA fault
matrix now covers 3 of the plan's 8 cases (corrupt image, interrupted write,
failed-boot rollback). Remaining OTA follow-ups: power-cut-after-write-before-
commit, gateway/BMS reset during update, and the two mode-gated cases
(OTA-while-driving / OTA-while-charging — these need OTA in the scenario
*schema*, shared with the deferred `toml_zoo` `ota-while-drive` case), plus the
OTA-rollback `debug_gym` seed (now trivially seedable from `gateway_ota_badcrc` /
`gateway_ota_badhealth`).

## Milestone 18 status (this branch)

The eighteenth milestone adds the OTA fault-matrix "power cut after write before
commit" case (the plan's fault-matrix item 3) — arguably the most important OTA
safety property: **commit atomicity**. It is **microcar-only**, opt-in, and
reuses the M16 slot model + M17 rollback machinery, so every existing golden
trace stays **byte-identical** (verified: the `debug_gym` hashes `c371b96e253e` /
`68806f23c980` / `16684074b01c` / `fa7f03709681` are unchanged, and the happy
path + the M16/M17 fault paths emit the exact same `ota_*` events).

- **Firmware.** New fault selector `OTA_FAULT_POWERCUT_PRECOMMIT`
  (`gateway_ota_powercut`): a valid image downloads and verifies (crc ok), but a
  power cut strikes at the commit step — instead of `mc_ota_commit()`, the model
  does `mc_ota_rollback()`, discarding the verified-but-uncommitted image and
  reverting to slot A: `IDLE(0) → DOWNLOADING(1) → VERIFYING(2, crc ok) →
  ROLLED_BACK(6, slot A)`. This proves the commit is the point of no return — a
  perfectly valid image still does NOT take effect if power is lost before it
  commits, so the vehicle keeps running its previous firmware. Wired via
  `microcar_boot_gateway_ota_powercut()` (coordinator) and the `src/lib.rs`
  resolver (`gateway_ota_powercut` matched before `gateway_ota`).
- **Harness / scenario.** No new expectation types — `dogfood/ota/
  rollback_powercut_precommit.toml` asserts `state-sequence 0,1,2,6` + **crc-ok**
  (the distinguishing marker vs the bad-CRC case, whose `0,1,2,6` carries
  crc-bad) + rolled-back + active-slot 0. Added 1 dogfood unit test (dogfood
  82 → **83**).

Verified locally: `cargo build --bin microcar` OK; `state_tests` **99** and the
dogfood crate **83** unit tests pass; `harness ota` is **5/5** green
(`happy_path`, `rollback_bad_crc`, `rollback_failed_health`,
`rollback_interrupted_write`, `rollback_powercut_precommit`); every other lane is
unregressed (`charging` 1/1, `diagnostics` 2/2, `debug-gym` 4/4 with unchanged
hashes, `topology` 7/7, `toml-zoo` 11/11); the touched Rust file is clippy-clean.
The OTA fault matrix now covers **4 of the plan's 8 cases** (corrupt image,
interrupted write, failed-boot rollback, power-cut-before-commit).

The remaining 4 fault-matrix cases are **decision-gated** and were intentionally
NOT done autonomously (to avoid guessing a modeling approach that could
compromise the goal): **gateway reset during update** and **BMS critical fault
during update** need a reliable cross-ECU / reset mechanism to model honestly
(the current CAN-RX path is still unreliable — a trace-only stub would be
fabricated rather than genuinely exercised); and **OTA-while-driving** /
**OTA-while-charging** are mode-gated — they need OTA represented in the scenario
*schema* (a design decision shared with the deferred `toml_zoo` `ota-while-drive`
case). The OTA-rollback `debug_gym` seed is now trivially seedable from any of
the four fault variants once the seeded-bug corpus format is agreed.

## Milestone 19 status (this branch)

The nineteenth milestone delivers the **first debug_gym seeded-bug corpus case**
— the plan's `debug_gym` "OTA rollback bug" seed (UNBLOCKING.md §4 Strategy C,
now satisfiable because the OTA firmware exists after M15–M18). It flips
BLOCKERS §2.2 from fully BLOCKED to "first case delivered". It is
**microcar-only**, gated behind opt-in buggy firmware, so every existing golden
trace stays **byte-identical** (verified: the `debug_gym` hashes `c371b96e253e` /
`68806f23c980` / `16684074b01c` / `fa7f03709681` are unchanged, and the OTA lane
still emits the exact same `ota_*` events — `ota` stays 5/5).

The seed is **genuinely exercised, not fabricated**: it pairs a real buggy
firmware variant with the real correct firmware and produces both traces through
the product binary.

- **Buggy firmware.** A new opt-in variant `gateway_ota_crcbug`
  (`gateway_enable_dogfood_ota_bug_bad_crc()` → `g_ota_crc_check_bug`, default
  `0`): the same corrupt-image campaign as `gateway_ota_badcrc`, but the
  gateway's CRC check is **broken** — it reports the corrupt image as valid, so
  the slot model commits and boots the bad slot instead of rolling back:
  `IDLE(0) → DOWNLOADING(1) → VERIFYING(2, crc wrongly OK) → COMMIT_PENDING(3) →
  REBOOTING(4) → HEALTHY(5)`, `ota_boot_result=1`. Wired via
  `microcar_boot_gateway_ota_crcbug()` (coordinator) and the `src/lib.rs`
  resolver (`gateway_ota_crcbug` matched **before** `gateway_ota`). The single
  seeded-bug line (`if (g_ota_crc_check_bug) crc_ok = 1;`) is off by default, so
  the default gateway firmware and every other lane are byte-identical. The fixed
  reference is the M16 `gateway_ota_badcrc`, which reports `ota_crc_ok=0` and
  rolls back to slot A (`… → ROLLED_BACK(6)`, `ota_active_slot=0`).
- **Corpus harness.** `dogfood/src/debug_gym_corpus.rs` +
  `harness debug-gym-corpus`. Each seed carries the plan's required metadata
  (description, symptom, minimal failing scenario, golden failing trace, required
  debugging primitive, fixed trace) and runs its **failing** (buggy) and
  **fixed** scenarios through the product binary
  (`dogfood/debug_gym/ota_rollback_bug/{failing,fixed}.toml`), asserting three
  things: **bug-reproduced** (the buggy firmware boots the corrupt image with no
  rollback), **bug-fixed** (the correct firmware rolls back to slot A and never
  boots it), and **traces-diverge** (the runs split at the VERIFYING step —
  `ota_crc_ok` 1 vs 0, ending `HEALTHY(5)` vs `ROLLED_BACK(6)`), which is exactly
  what the documented primitive (`continue_until(ota_state)` + inspect
  `ota_crc_ok`) localizes.

Verified locally: `cargo build --bin microcar` OK; the dogfood crate now has
**88 unit tests** (83 → 88, +5 corpus) and `state_tests` **99**, all passing;
`harness debug-gym-corpus` is **1/1** green; every other lane is unregressed
(`debug-gym` 4/4 with unchanged hashes, `ota` 5/5, `diagnostics` 2/2,
`charging` 1/1, `topology` 7/7, `toml-zoo` 11/11); and the new Rust module is
clippy/fmt-clean. Remaining debug_gym corpus seeds (gateway race, powertrain
timeout/cancel, BMS stale sensor, dashboard missed warning, telematics
partial-write, gateway bridge loop) reuse this harness — the diagnostics-seeded
ones (UNBLOCKING §4 Strategy A) are the natural next additions; telematics is
still firmware-gated (Strategy C).

## Milestone 20 status (this branch)

The twentieth milestone adds the **second debug_gym seeded-bug corpus case** —
the diagnostics-seeded **SERVICE-mode torque-clamp bug** (UNBLOCKING.md §4
Strategy A, "start with diagnostics bugs"), reusing the M19 `debug-gym-corpus`
harness. **microcar-only**, gated behind opt-in buggy firmware, so every existing
golden trace stays **byte-identical** (verified: `debug_gym` hashes
`c371b96e253e` / `68806f23c980` / `16684074b01c` / `fa7f03709681` unchanged, and
`diagnostics` stays 2/2 — the default powertrain SERVICE firmware is untouched).

Genuinely exercised, not fabricated — a real buggy firmware variant paired with
the real correct firmware:

- **Buggy firmware.** A new opt-in powertrain variant
  `powertrain_diag_service_bug` (`powertrain_enable_dogfood_service_clamp_bug()`
  → `g_diag_service_clamp_bug`, default `0`): it runs a SERVICE-mode torque
  computation but **skips the safety clamp**, so an 80% throttle demand during a
  service session still commands drive torque with the motor enabled
  (`diag_motor_command` = torque 80 / motor_enable 1). The single seeded-bug
  branch is gated and off by default; the trace guard keeps `vehicle_mode ==
  SERVICE`, so the fix's own trace path is reused. The fixed reference is the M12
  `powertrain_diag_service`, whose `mc_safety_clamp_torque` forces torque to 0
  and `safety_mode_blocks_torque` disables the motor (`diag_motor_command` = 0).
  Wired via `microcar_boot_powertrain_diag_service_bug()` (coordinator) and the
  `src/lib.rs` resolver (`powertrain_diag_service_bug` matched **before**
  `powertrain_diag_service`).
- **Corpus.** `dogfood/debug_gym/service_torque_bug/{failing,fixed}.toml` (based
  on the diagnostics `service_mode_disables_drive` scenario, swapping only the
  powertrain firmware) + a new `SeedKind::ServiceTorqueClamp` in
  `debug_gym_corpus.rs` that decodes the packed `diag_motor_command` trace
  (`(torque<<8)|motor_enable`) and asserts **bug-reproduced** (SERVICE motor
  command with torque>0 / motor enabled), **bug-fixed** (every SERVICE command
  clamped to torque 0 / motor disabled), and **traces-diverge** (buggy max torque
  80 vs fixed 0) — localizable by breakpointing on the `diag_motor_command`
  trace.

Verified locally: `cargo build --bin microcar` OK; the dogfood crate now has
**92 unit tests** (88 → 92, +4) and `state_tests` **99**, all passing;
`harness debug-gym-corpus` is **2/2** green (`ota_rollback`, `service_torque`);
every other lane is unregressed (`debug-gym` 4/4 with unchanged hashes,
`diagnostics` 2/2, `ota` 5/5, `charging` 1/1, `topology` 7/7, `toml-zoo` 11/11);
the new Rust is clippy/fmt-clean. The debug_gym corpus now covers 2 of the
plan's 7 seeds (OTA rollback, SERVICE torque clamp); the remaining
diagnostics-seeded cases (clears-all-DTCs, START_SESSION-succeeds-in-DRIVE) reuse
this same harness, and telematics/OTA-network seeds stay firmware-gated.

## Milestone 21 status (this branch)

The twenty-first milestone adds the **third debug_gym seeded-bug corpus case** —
the diagnostics-seeded **clear-all-DTCs bug** (UNBLOCKING.md §4 Strategy A,
"start with diagnostics bugs" — the `clears-all-DTCs` candidate), reusing the
M19 `debug-gym-corpus` harness. **microcar-only**, gated behind opt-in buggy
firmware, so every existing golden trace stays **byte-identical** (verified:
`debug_gym` hashes `c371b96e253e` / `68806f23c980` / `16684074b01c` /
`fa7f03709681` unchanged, and `diagnostics` stays 2/2 — the default diag/clear
firmware path is untouched).

Genuinely exercised, not fabricated — a real buggy firmware variant paired with
the real correct firmware, both run through the product binary:

- **Slot for the bug.** `common`-adjacent gateway fault manager gained a genuine
  `fault_manager_clear_all()` API (distinct from the scoped
  `fault_manager_clear_node()`), with a header note that a subsystem-scoped
  "clear DTCs" must NOT call it. This is the exact function the buggy firmware
  wrongly reaches.
- **Shared setup.** A new `gateway_enable_dogfood_diag_clear_dtcs(buggy)` runs
  the existing diag request script but injects **two** DTCs before CLEAR_DTCS: a
  BMS overtemp fault (at 300 ms, via `synth_bms_fault`) *and* an unrelated
  powertrain fault (at 310 ms, via a new mutex-guarded `synth_report_fault`
  helper writing `MC_NODE_POWERTRAIN` / `MC_WARN_POWERTRAIN_OFFLINE`). So when
  CLEAR_DTCS runs, the scoping actually matters — a BMS-scoped clear must leave
  the powertrain DTC behind.
- **Buggy firmware** (`gateway_diag_clearbug`,
  `g_diag_clear_all_bug`, default `0`): the CLEAR_DTCS handler calls
  `fault_manager_clear_all(&g_fm)` instead of
  `fault_manager_clear_node(&g_fm, MC_NODE_BMS)`, so a BMS-scoped clear wrongly
  drops the powertrain DTC too. The follow-up READ_DTCS reports 0. The single
  seeded-bug branch is gated and off by default. The **fixed** reference
  (`gateway_diag_clear`) clears only the BMS node, so the follow-up READ_DTCS
  reports 1 (the powertrain fault survives). Both wired via
  `microcar_boot_gateway_diag_clear{,bug}()` (coordinator) and the `src/lib.rs`
  resolver (`gateway_diag_clearbug` matched **before** `gateway_diag_clear`,
  both **before** `gateway_diag_fault` / `gateway_diag`).
- **Corpus.** `dogfood/debug_gym/clear_dtcs_bug/{failing,fixed}.toml` (based on
  the diagnostics `read_clear_dtcs_after_bms_fault` scenario, swapping only the
  gateway firmware) + a new `SeedKind::ClearAllDtcs` in `debug_gym_corpus.rs`
  that decodes the packed `gateway_diag_response` head
  (`(req<<24)|(service<<16)|(status<<8)|value0`) to read the DTC count of the
  pre-clear (req 3) and post-clear (req 5) READ_DTCS responses. It asserts
  **bug-reproduced** (buggy: 2 DTCs before, 0 after — the powertrain fault
  silently dropped), **bug-fixed** (fixed: 2 before, 1 after — only BMS cleared),
  and **traces-diverge** (post-clear count 0 vs 1) — localizable by
  breakpointing on the post-clear `gateway_diag_response` trace.

Verified locally: `cargo build --bin microcar` OK; the dogfood crate now has
**96 unit tests** (92 → 96, +4) and `state_tests` **99**, all passing;
`harness debug-gym-corpus` is **3/3** green (`ota_rollback`, `service_torque`,
`clear_all_dtcs`); every other lane is unregressed (`debug-gym` 4/4 with
unchanged hashes, `diagnostics` 2/2, `ota` 5/5, `charging` 1/1, `topology` 7/7,
`toml-zoo` 11/11, `simfarm` PASS); the new Rust is clippy/fmt-clean. The
debug_gym corpus now covers 3 of the plan's 7 seeds (OTA rollback, SERVICE
torque clamp, clear-all-DTCs); the remaining diagnostics-seeded case
(START_SESSION-succeeds-in-DRIVE) reuses this same harness, and the
telematics/OTA-network/BMS-stale-sensor/dashboard-missed-warning seeds stay
firmware-gated.

## Milestone 22 status (this branch)

The twenty-second milestone adds the **fourth debug_gym seeded-bug corpus case**
— the diagnostics-seeded **START_SESSION-in-DRIVE bug** (UNBLOCKING.md §4
Strategy A, "start with diagnostics bugs" — the last named candidate), reusing
the M19 `debug-gym-corpus` harness. **microcar-only**, gated behind opt-in buggy
firmware, so every existing golden trace stays **byte-identical** (verified:
`debug_gym` hashes `c371b96e253e` / `68806f23c980` / `16684074b01c` /
`fa7f03709681` unchanged, and `diagnostics` stays 2/2 — the default diag firmware
path is untouched).

Genuinely exercised, not fabricated — a real buggy firmware variant paired with
the real correct firmware, both run through the product binary:

- **The guard under test.** The gateway's `handle_diag_request` START_SESSION
  case already refuses a session while `g_gs.mode == VEHICLE_DRIVE` (safety: no
  service session mid-drive). The reject branch now also reports the current
  mode in `value0` (previously left 0) so a rejected response carries DRIVE —
  this only affects a response that is *never emitted* by existing scenarios (they
  send START_SESSION at a non-DRIVE mode), so all existing lanes stay
  byte-identical.
- **Script.** A new `gateway_enable_dogfood_diag_startdrive(buggy)` +
  `run_dogfood_diag_startdrive_script`: at 100 ms the vehicle is put in DRIVE (a
  dogfood trigger, exactly like the M13 charging script's plug event) and a
  diagnostic tool attempts START_SESSION mid-drive; at 200 ms a READ_MODE reads
  back the mode. The mode is forced in the *same tick* immediately before the
  request, so `update_vehicle_mode` (which runs later in the loop) cannot perturb
  the mode the handler observes.
- **Buggy firmware** (`gateway_diag_startdrivebug`, `g_diag_startsession_drive_bug`,
  default `0`): the DRIVE guard is skipped, so the mid-drive START_SESSION is
  *accepted* (`gateway_diag_response` status=OK, mode=SERVICE) — the vehicle
  drops out of DRIVE into SERVICE while moving. The **fixed** reference
  (`gateway_diag_startdrive`) rejects it (status=REJECTED, mode=DRIVE). Both wired
  via `microcar_boot_gateway_diag_startdrive{,bug}()` (coordinator) and the
  `src/lib.rs` resolver (`gateway_diag_startdrivebug` matched **before**
  `gateway_diag_startdrive`, both **before** `gateway_diag_fault` /
  `gateway_diag`).
- **Corpus.** `dogfood/debug_gym/start_session_drive_bug/{failing,fixed}.toml` +
  a new `SeedKind::StartSessionInDrive` in `debug_gym_corpus.rs` that decodes the
  START_SESSION `gateway_diag_response` `(status, value0=mode)` and asserts
  **bug-reproduced** (buggy: status=OK, mode=SERVICE — accepted mid-drive),
  **bug-fixed** (fixed: status=REJECTED, mode=DRIVE — refused, still driving), and
  **traces-diverge** (status OK vs REJECTED) — localizable by breakpointing on the
  START_SESSION `gateway_diag_response`.

Verified locally: `cargo build --bin microcar` OK; the dogfood crate now has
**100 unit tests** (96 → 100, +4) and `state_tests` **99**, all passing;
`harness debug-gym-corpus` is **4/4** green (`ota_rollback`, `service_torque`,
`clear_all_dtcs`, `start_session_in_drive`); every other lane is unregressed
(`debug-gym` 4/4 with unchanged hashes, `diagnostics` 2/2, `ota` 5/5,
`charging` 1/1, `topology` 7/7, `toml-zoo` 11/11); the new Rust is
clippy/fmt-clean. The debug_gym corpus now covers **4 of the plan's 7 seeds** (OTA
rollback, SERVICE torque clamp, clear-all-DTCs, START_SESSION-in-DRIVE) — all four
diagnostics/OTA-seeded cases reachable without new firmware are done. The
remaining 3 seeds (BMS stale sensor, dashboard missed warning, telematics
partial-write) stay firmware-gated (Strategy C); the gateway-bridge-loop bug is
already exercised structurally by the topology `gateway_loop_prevention` scenario.

## Assumptions

- `microcar/docs/costar_microcar_dogfood_plan.md` is the canonical planning document.
- Existing docs are not replaced by this file yet.
- The task list intentionally includes both `costar` and `microcar` tasks because the dogfood value depends on both repos.
- The first implementation milestone should be `costar` stabilization plus `microcar` harness foundation, not cockpit or OTA.
- High-fidelity vehicle physics, AUTOSAR, ISO 26262 process modeling, real CAN bit timing, and production bootloader/security behavior are out of scope.
