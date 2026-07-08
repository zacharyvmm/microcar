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

## Assumptions

- `microcar/docs/costar_microcar_dogfood_plan.md` is the canonical planning document.
- Existing docs are not replaced by this file yet.
- The task list intentionally includes both `costar` and `microcar` tasks because the dogfood value depends on both repos.
- The first implementation milestone should be `costar` stabilization plus `microcar` harness foundation, not cockpit or OTA.
- High-fidelity vehicle physics, AUTOSAR, ISO 26262 process modeling, real CAN bit timing, and production bootloader/security behavior are out of scope.
