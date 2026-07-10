# costar + microcar Compact-EV Dogfood Plan

## Summary

This plan turns `microcar` into the canonical dogfood project for `costar`: a compact passenger-EV embedded-system benchmark that exercises the simulator surfaces the product needs most.

> **Implementation-status correction (2026-07-10):** M23-M27 landed staged
> primitives, not accepted P0a/P0b/P1 infrastructure. The required repair order
> and acceptance tests live in [`../UNBLOCKING.md`](../UNBLOCKING.md); do not
> treat a trace-only CAN or reboot result as a completed dogfood lane.

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

## Task List

### costar stabilization

- [ ] Add `run_until` regression tests for cancelled tombstones.
- [ ] Fix `run_until` so cancelled events cannot cause `unwrap()` panics.
- [ ] Fix `run_until` so live events beyond the deadline are not dispatched.
- [ ] Add multi-fiber resume/suspend regression coverage.
- [ ] Refresh the active fiber yielder on every resume.
- [ ] Clear or restore the active fiber yielder on scheduler return.
- [ ] Add stepped-vs-continuous equivalence tests.
- [ ] Guard FFI paths so guest-reachable panics do not cross `extern "C"` boundaries.

### costar per-session state

- [ ] Inventory all process-global and thread-local simulator state.
- [ ] Define the per-World/per-simulator state owner.
- [ ] Move device registries into per-session/per-World state.
- [ ] Move virtual clock and current task identity into active simulator state.
- [ ] Move display/touch inspection state into the session-owned world.
- [ ] Update gRPC `ConfigureBoard` to write session-owned device state.
- [ ] Update gRPC `Run` to execute against the same device state.
- [ ] Update `InspectDevices` to read session-owned device state.
- [ ] Add concurrent-session determinism tests.

### costar control planes

- [ ] Define a shared world runner used by JSON-RPC and gRPC.
- [ ] Avoid holding the global sessions lock during full simulation runs.
- [ ] Return the world to the session on successful run completion.
- [ ] Mark the session failed and recover cleanly after run panic.
- [ ] Add deterministic session listing.
- [ ] Bound session/keyframe/trace storage or add cleanup policies.
- [ ] Add gRPC integration test for configure/run/display/touch/inspect.
- [ ] Add JSON-RPC panic-isolation test.
- [ ] Add gRPC panic-isolation test.

### costar trace, topology, and debugging

- [ ] Define trace v2 event structure.
- [ ] Add machine/component/port identity to trace events.
- [ ] Add tx-to-rx correlation IDs.
- [ ] Add parent/child correlation for gateway forwarding.
- [ ] Add compatibility adapter for legacy human/JSONL trace output.
- [ ] Define topology graph model with nodes, ports, and typed edges.
- [ ] Support multi-interface machines.
- [ ] Add `step_event`.
- [ ] Add `step_message`.
- [ ] Add `step_task`.
- [ ] Add `continue_until(predicate)`.
- [ ] Add breakpoint predicates for messages, vehicle state, device state, dropped frames, DTC creation, and assertion failure.
- [ ] Implement keyframe save/restore.
- [ ] Add deterministic replay/scrub support.

### costar networking and devices

- [ ] Fix `sim_net_drain_tx` to preserve all queued TX frames.
- [ ] Add frame-conservation tests.
- [ ] Re-arm host poller fds after readiness events.
- [ ] Add repeated-host-I/O wakeup integration test.
- [ ] Fix TCP bridge partial-write framing.
- [ ] Add socket-pressure integration test.
- [ ] Guard zero-length SPI RX fault-injection corruption.
- [ ] Add FFI panic-boundary tests for device paths.

### microcar foundation

- [ ] Update README language from go-kart to compact passenger EV.
- [ ] Update architecture docs to describe compact-EV topology and modes.
- [ ] Add vehicle model doc.
- [ ] Add roadmap doc linking dogfood lanes to `costar` capabilities.
- [ ] Keep current Gateway, Powertrain, BMS, and Dashboard ECUs as the base.
- [ ] Define current and future vehicle modes in docs and scenarios.
- [ ] Create `dogfood/harness`.
- [ ] Add scenario runner wrapper.
- [ ] Add trace hash normalizer.
- [ ] Add invariant checker skeleton.
- [ ] Add timeout wrapper.
- [ ] Add JSON summary output.
- [ ] Add solo-vs-repeat determinism check.

### microcar simfarm

- [ ] Add concurrent-session runner.
- [ ] Add solo-vs-concurrent trace hash comparison.
- [ ] Add `N=2` smoke scenario.
- [ ] Add `N=4` medium-lane scenario.
- [ ] Add `N=8` and `N=16` nightly scenarios.
- [ ] Add session churn test.
- [ ] Add RSS plateau check.
- [ ] Add intentionally bad scenario for panic isolation.
- [ ] Assert healthy sibling sessions finish unchanged.

### microcar toml_zoo

- [ ] Add duplicate machine ID case.
- [ ] Add missing gateway case.
- [ ] Add bad bus reference case.
- [ ] Add unknown ECU firmware case.
- [ ] Add invalid fault target case.
- [ ] Add negative duration case.
- [ ] Add huge duration case.
- [ ] Add duplicate bus node case.
- [ ] Add drive mode without powertrain case.
- [ ] Add charging while drive case.
- [ ] Add OTA while drive case.
- [ ] Assert all cases return structured errors.
- [ ] Assert malformed cases do not poison server sessions.

### microcar topology

- [ ] Add `vcan_drive`, `vcan_body`, and `vcan_diag` scenarios.
- [ ] Add body-control ECU stub only as needed for body-bus coverage.
- [ ] Add diagnostic-tool ECU or host diagnostic client.
- [ ] Add gateway bridge rules.
- [ ] Add gateway loop-prevention behavior.
- [ ] Add topology invariant checker.
- [ ] Add 16-node fleet scenario.
- [ ] Add 64-node fleet scenario.

### microcar cockpit

- [ ] Define dashboard framebuffer contract.
- [ ] Add boot screen state.
- [ ] Add ready screen state.
- [ ] Add drive screen state.
- [ ] Add limited-power warning screen state.
- [ ] Add fault screen state.
- [ ] Add charging screen state later.
- [ ] Add OTA progress screen state later.
- [ ] Add gRPC cockpit harness.
- [ ] Add framebuffer hash checkpoints.
- [ ] Assert display state matches trace-derived vehicle state.
- [ ] Assert touch injection causes expected UI transition.

### microcar debug_gym

- [ ] Add seeded gateway race bug.
- [ ] Add seeded powertrain timeout/cancel bug.
- [ ] Add seeded BMS stale sensor bug.
- [ ] Add seeded dashboard missed warning bug.
- [ ] Add seeded gateway bridge loop bug.
- [ ] Add bug metadata format.
- [ ] Add failing trace fixtures.
- [ ] Add fixed trace fixtures.
- [ ] Add stepped-vs-continuous checks.
- [ ] Add keyframe restore checks.
- [ ] Add breakpoint acceptance checks.

### microcar diagnostics

- [ ] Add DTC state model.
- [ ] Add diagnostic session start.
- [ ] Add read vehicle mode flow.
- [ ] Add read DTC flow.
- [ ] Add clear DTC flow.
- [ ] Add live BMS data flow.
- [ ] Add actuator self-test flow.
- [ ] Add service mode disables drive scenario.
- [ ] Assert DTC persists after transient fault.
- [ ] Assert clear DTC succeeds only when fault inactive.

### microcar telematics

- [ ] Add telematics ECU or host-connected telematics path.
- [ ] Add periodic status upload.
- [ ] Add remote diagnostic query.
- [ ] Add remote lock/unlock mock.
- [ ] Add remote precondition request.
- [ ] Add fault upload.
- [ ] Add host TCP test harness.
- [ ] Force small socket buffers in harness.
- [ ] Assert repeated wakeups work.
- [ ] Assert partial writes do not corrupt framing.
- [ ] Assert remote command is rejected in unsafe state.

### microcar charging

- [ ] Add charger ECU or charger environment model.
- [ ] Add plug inserted event.
- [ ] Add charging handshake.
- [ ] Add charging active state.
- [ ] Add BMS reduced-current request.
- [ ] Add charge complete state.
- [ ] Add drive rejected while plugged scenario.
- [ ] Add charger fault stops charging scenario.
- [ ] Add unplug mid-charge scenario.

### microcar OTA

- [ ] Add OTA slot model.
- [ ] Add image download state.
- [ ] Add slot B write state.
- [ ] Add CRC verification.
- [ ] Add boot flag commit.
- [ ] Add reboot and health check.
- [ ] Add rollback behavior.
- [ ] Add power-cut-before-write scenario.
- [ ] Add power-cut-during-write scenario.
- [ ] Add power-cut-after-write-before-commit scenario.
- [ ] Add corrupt image scenario.
- [ ] Add OTA rejected in DRIVE scenario.
- [ ] Add OTA rejected while charging scenario.

### CI

- [ ] Add PR-fast dogfood lane.
- [ ] Add medium/main dogfood lane.
- [ ] Add nightly dogfood lane.
- [ ] Add manual/demo checklist.
- [ ] Emit JSON summaries suitable for CI artifacts.
- [ ] Document expected runtime budget for each lane.

## M23-M27 Implementation Review Correction

The milestone notes above record the staged implementation and its local tests;
they do not clear the plan's isolation, CAN, or restart gates. Before advancing
to real diagnostics, charging, OTA reset, cockpit multi-session, or fleet
claims, implement the remediation gate in `../UNBLOCKING.md`:

1. Replace the public raw-pointer `DeviceBank` activation guard with a
   lifetime-safe scoped context, and audit the matching `SimGlobal` mechanism.
2. Give every production `Machine` an owned/provisioned device bank and bind all
   gRPC board, touch, display, and inspection calls to an explicit session World
   and machine.
3. Centralize all firmware stepping, receiver inbox staging, and sender TX
   draining in that active machine context; no `advance_to` path may bypass it.
4. Preserve immutable machine configuration and persistent storage across
   restart, wire factories in the microcar binary, and drop frames sent while a
   machine is down.
5. Prove the result with actual two-World, concurrent-gRPC, CAN-boundary, and
   microcar gateway-reboot tests, then retain existing byte-identical lanes as
   regressions.

The later migration of clock/task identity, network state, and C firmware
instance state remains separate. It is required before claiming concurrent
duplicate-ECU or fleet isolation, but it must not be used as a reason to defer
the B0-B3 remediation above.

## Assumptions

- `microcar/docs/costar_microcar_dogfood_plan.md` is the canonical planning document.
- Existing docs are not replaced by this file yet.
- The task list intentionally includes both `costar` and `microcar` tasks because the dogfood value depends on both repos.
- The first implementation milestone should be `costar` stabilization plus `microcar` harness foundation, not cockpit or OTA.
- High-fidelity vehicle physics, AUTOSAR, ISO 26262 process modeling, real CAN bit timing, and production bootloader/security behavior are out of scope.
