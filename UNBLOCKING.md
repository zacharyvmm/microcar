# Microcar Dogfood Unblocking Strategy

Last updated: 2026-07-08 (after the M13 charging safety lane)

This report is a playbook for the remaining blockers in the costar/microcar
dogfood plan. It assumes the current state after the diagnostics unblock and the
M13 charging safety lane:

- `harness diagnostics` is green for the first diagnostics dogfood lane.
- `harness charging` is green for the first charging safety lane (drive blocked
  while plugged) — see section 1, Strategy A: its success criteria are now met.
- `sim-grpc` is locally buildable by exporting
  `PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc`.
- Some firmware CAN RX/TX behavior is still not reliable enough for
  product-grade diagnostics-over-bus assertions, so the diagnostics and charging
  lanes use explicit dogfood firmware variants and firmware trace events.

The goal is not to prescribe one implementation. It is to give the next agent
several practical debugging strategies for each blocker, plus success criteria
that make "unblocked" measurable.

## Ground Rules

1. Keep existing green lanes green after every stage:
   - `cargo build --bin microcar`
   - `cargo test --manifest-path dogfood/Cargo.toml`
   - `MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- diagnostics`
2. Prefer a small dogfood lane or one scenario before broad refactors.
3. Do not depend on full golden traces for new volatile lanes. Assert compact
   semantic trace events or structured JSON reports first.
4. When a lane touches simulator state ownership, isolate the minimal simulator
   bug before changing firmware.
5. Treat `docs/BLOCKERS.md` as the status source and update it whenever a
   blocker changes state.

## Current Remaining Blockers

1. Charging firmware lane — **safety lane done (M13)**; deeper FSM + plant physics remain.
2. OTA firmware lane.
3. Diagnostics depth and product-grade diagnostics-over-bus.
4. Debug gym seeded-bug corpus.
5. Cockpit/gRPC dogfood lane.
6. Telematics firmware and host-socket lane.
7. Per-session state ownership refactor.
8. Remaining breakpoint predicates and nightly/scale runs.

## 1. Charging Firmware Lane

### Status (M13): safety lane delivered

Strategy A below was taken and its success criteria are met: `harness charging`
has a passing `plug_blocks_drive` scenario, `cargo test --manifest-path
dogfood/Cargo.toml` stays green (68 tests), and existing non-charging scenarios
are byte-identical (verified via the `debug_gym` trace hashes). Delivered
trace-backed via dogfood firmware variants (`gateway_charging`,
`powertrain_charging`) — no reliance on firmware CAN RX. It asserts
`charging-mode` (gateway entered CHARGING on plug-in), `drive-blocked` (a drive
request while plugged stayed CHARGING), and `motor-torque max=0` (powertrain
clamped torque to 0 with the motor disabled).

Key finding: `mc_gateway_determine_mode` already keeps `VEHICLE_CHARGING` sticky
and `safety_mode_blocks_torque` already blocks torque outside DRIVE/LIMP — the
gap was only a *trigger* to enter CHARGING and a lane to exercise it.

**Still open for a future milestone** (Strategy C is the natural next step): the
richer charging FSM (handshake, temperature-rise → reduced-current, charge
complete, fault stops charging) and charging plant physics, plus flipping the
deferred `toml_zoo` `charging-while-drive` case to active (needs charging in the
scenario *schema*, e.g. a plug input / mode field).

### Blocker

The plan expects charging states and safety behavior that are not implemented
yet: plug detection, handshake, charging, temperature/current limiting,
completion, and "drive blocked while plugged".

### Strategy A: Scenario-First Contract

Use this when the state model is not settled.

Steps:

1. Add one minimal scenario, for example
   `dogfood/charging/plug_blocks_drive.toml`.
2. Express expectations as comments similar to diagnostics:
   `# charging-expect: mode=CHARGING`, `# charging-expect: torque max=0`.
3. Add a std-only `harness charging` parser that reports semantic checks.
4. Stub the firmware traces first:
   - gateway traces `charging_state`
   - powertrain traces `charging_motor_command`
   - BMS traces `charging_current_limit`
5. Implement the smallest firmware behavior that makes the scenario pass.

Debugging tactics:

- If the gateway never enters `CHARGING`, trace the plug event and gateway mode
  decision separately.
- If powertrain still produces torque, trace both raw requested torque and
  clamped torque.
- If BMS limits do not affect charging current, add a direct BMS state unit test
  before debugging the scenario.

Success criteria:

- `harness charging` has at least one passing plug-blocks-drive scenario.
- `cargo test --manifest-path dogfood/Cargo.toml` stays green.
- Existing non-charging scenarios do not change golden traces unless expected.

### Strategy B: Firmware State Machine First

Use this when the team has already agreed on charging states.

Steps:

1. Add a compact charging enum and transition table in gateway state code.
2. Unit-test the transition table in `state_tests` or an equivalent small Rust
   test.
3. Wire the gateway mode broadcast.
4. Add powertrain clamp behavior for `VEHICLE_CHARGING`.
5. Only then add dogfood scenarios.

Debugging tactics:

- Table-test every transition before running the full simulator.
- Make invalid transitions trace `charging_reject_reason`.
- Keep gateway as sole charging mode authority.

Success criteria:

- State tests cover parked plug-in, drive request while plugged, charge complete,
  and fault during charge.
- Dogfood scenarios confirm the same behavior under FreeRTOS scheduling.

### Strategy C: Plant/Battery-Centric Debugging

Use this when charging current, SOC, or temperature behavior is the risk.

Steps:

1. Extend the plant model to expose charger input and battery temperature trend.
2. Add a deterministic charging current profile.
3. Add BMS current-limit traces.
4. Assert SOC increases and temperature limiting reduces current.

Debugging tactics:

- Check plant tick order before blaming firmware.
- Log `soc_percent`, `pack_temp_c_x10`, and current limit in the same virtual
  time window.
- Keep battery physics deliberately simple until the control-flow lane is green.

Success criteria:

- Charging current falls when high-temperature condition is injected.
- SOC/charge-complete assertions are deterministic across repeated runs.

## 2. OTA Firmware Lane

### Blocker

OTA requires new firmware and boot behavior: download, slot-B write, CRC,
commit, reboot, health check, rollback, and an eight-case power-cut/corruption
matrix.

### Strategy A: Happy Path First

Use this to avoid being buried by the fault matrix.

Steps:

1. Define the smallest OTA state sequence:
   `IDLE -> DOWNLOADING -> VERIFYING -> COMMIT_PENDING -> REBOOTING -> HEALTHY`.
2. Trace `ota_state`, `ota_slot`, `ota_crc_ok`, and `ota_boot_result`.
3. Add one `dogfood/ota/happy_path.toml`.
4. Add `harness ota` with semantic assertions.
5. Add rollback only after the happy path is green.

Debugging tactics:

- If state order is wrong, assert a monotonic expected sequence in the harness.
- If CRC behavior is flaky, unit-test CRC over fixed byte arrays outside the
  simulator first.
- If reboot does not occur, trace boot flags before and after restart.

Success criteria:

- Happy-path OTA scenario passes with deterministic state sequence.
- Existing lifecycle/reboot scenarios still pass.

### Strategy B: Storage/Slot Model First

Use this when the risk is persistence or slot ownership.

Steps:

1. Create a pure slot metadata model: active slot, pending slot, rollback flag,
   health counter.
2. Unit-test commit and rollback rules without FreeRTOS.
3. Add firmware shims that call the model.
4. Add simulator scenarios only after the model is stable.

Debugging tactics:

- Print or trace a packed `ota_flags` value after every transition.
- Test corrupted image, missing commit flag, and failed health check as unit
  cases first.
- Avoid adding network/download behavior until slot rules are correct.

Success criteria:

- Slot model tests cover commit, rollback, corrupt CRC, and interrupted write.
- Dogfood OTA scenarios reuse the same model behavior.

### Strategy C: Fault-Matrix Driven

Use this after the happy path exists.

Steps:

1. Build the eight failure scenarios one at a time.
2. For each scenario, add:
   - trigger time
   - expected OTA state
   - expected slot after reboot
   - expected trace marker
3. Keep each failing path small enough to debug independently.

Debugging tactics:

- Use `World::step()` or `microcar --step` to stop near the fault injection.
- Use `run_to_frame` only for message-level bugs; use state traces for OTA
  state bugs.
- When rollback fails, compare pre-reboot and post-reboot `ota_flags`.

Success criteria:

- Fault matrix passes in the OTA harness.
- `ota-while-drive` can move from deferred to active `toml_zoo` coverage.

## 3. Diagnostics Depth And Diagnostics-Over-Bus

### Blocker

The first diagnostics dogfood lane is green, but live BMS data is not
implemented. Also, firmware-originated CAN and firmware RX are not currently a
reliable assertion path for this lane, so the lane uses dogfood firmware variants
and trace events.

### Strategy A: Live BMS Over Existing Trace Hooks

Use this when the priority is plan coverage rather than simulator CAN repair.

Steps:

1. Add `MC_DIAG_LIVE_BMS` support in gateway diagnostics.
2. Return a compact BMS snapshot from gateway-maintained state.
3. Add `# diagnostics-expect: live-bms ...` directives.
4. Assert gateway trace-backed responses in `harness diagnostics`.

Debugging tactics:

- First trace whether gateway ever receives or synthesizes BMS state.
- Pack BMS values in two response bytes initially; avoid multi-frame protocol
  until needed.
- Add unit tests for response decoding in `dogfood/src/diagnostics.rs`.

Success criteria:

- Diagnostics harness checks live BMS data in at least one scenario.
- No dependence on flaky firmware CAN RX/TX.

### Strategy B: Minimal CAN RX/TX Repro

Use this when the priority is product-grade diagnostics-over-bus.

Steps:

1. Create the smallest simulator scenario with two firmware machines:
   one sends one CAN frame, one receives one CAN frame.
2. Add temporary trace events around `sim_can_send`, world bus delivery, and
   `sim_can_recv`.
3. Prove whether the frame is lost, consumed by the wrong machine, or stuck in a
   global queue.
4. Fix the simulator integration before changing diagnostics firmware.

Debugging tactics:

- Compare human `can-rx`/`can-tx` trace against firmware `can_recv` values.
- Inspect whether controller `0` is process-global, thread-local, or
  machine-local at the point of delivery.
- Add a regression test in costar if the bug is in simulator ownership.

Success criteria:

- A firmware receiver reliably consumes the frame intended for it.
- Diagnostics scenarios can be rewritten to assert actual `0x600`/`0x601`
  delivery instead of only trace-backed hooks.

### Strategy C: Hybrid Transition

Use this when dogfood must stay green while the simulator fix is in progress.

Steps:

1. Keep the existing trace-backed diagnostics lane.
2. Add one skipped or expected-failing "over-bus" scenario documenting the gap.
3. Flip it to active after the CAN fix lands.

Debugging tactics:

- Do not remove trace-backed assertions until the over-bus lane has repeated-run
  stability.
- Keep response payload expectations identical between both harness paths.

Success criteria:

- Both trace-backed and bus-backed diagnostics lanes pass, then retire the
  trace-only dogfood variant if product requirements demand it.

## 4. Debug Gym Seeded-Bug Corpus

### Blocker

The debugging primitives are implemented, but the seeded-bug corpus needs
deliberately buggy firmware variants and expected failing/fixed traces.

### Strategy A: Start With Diagnostics Bugs

Use this now because diagnostics has dogfood firmware variants.

Candidate bugs:

- Gateway clears all DTCs instead of only BMS DTCs.
- `START_SESSION` incorrectly succeeds while in `DRIVE`.
- SERVICE mode fails to clamp powertrain torque.

Debugging tactics:

- Create `firmware/*_bug_*` variants or resolver-selected bug modes.
- Record the failing symptom in the harness JSON.
- Use `continue_until` on `gateway_diag_response` or `diag_motor_command`.

Success criteria:

- At least one debug-gym corpus case has failing and fixed traces.
- The corpus documents which primitive solved the case.

### Strategy B: Reuse Existing Topology Bridge Loop

Use this for a simulator/networking-flavored seed.

Steps:

1. Treat `gateway_loop_prevention` as the fixed scenario.
2. Add a deliberately broken bridge-loop variant or fixture.
3. Use Trace v2 parent/correlation IDs as the debugging signal.

Debugging tactics:

- Assert duplicate deliveries and parent-chain growth.
- Use `run_to_frame` to stop at the repeated message ID.

Success criteria:

- One seeded bug proves message breakpoint and Trace v2 causality debugging.

### Strategy C: Wait For OTA/Telematics For Later Seeds

Use this to avoid inventing shallow bugs.

Steps:

1. Mark OTA rollback and telematics partial-write bugs as dependent on those
   lanes.
2. Add them only after the real firmware behavior exists.
3. Keep corpus metadata ready: name, symptom, primitive, fixed trace.

Debugging tactics:

- Do not hand-roll fake networking bugs before telematics firmware exists.
- Make each seed fail for exactly one reason.

Success criteria:

- Seven-case corpus eventually spans gateway, powertrain, BMS, dashboard,
  telematics, OTA, and topology.

## 5. Cockpit / gRPC Dogfood Lane

### Blocker

The old `protoc` blocker is locally resolved. The remaining work is a lane
implementation around `sim-grpc`.

### Strategy A: Prove The Existing gRPC Surface

Use this as the first path.

Steps:

1. Run:
   `PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc cargo test -p sim-grpc`
   from `/home/zmm/projects/costar`.
2. Create a small client flow:
   `CreateSession -> ConfigureBoard -> Run(stream_display=true) -> InspectDevices`.
3. Add `harness cockpit` only after the manual/client flow is stable.

Debugging tactics:

- If tests fail in sandbox due to localhost binding, rerun with approval outside
  the sandbox.
- If generated proto code is stale, clean only the relevant costar build output.
- Log the gRPC request sequence before adding display assertions.

Success criteria:

- A dogfood cockpit run can create a session and inspect configured devices.

### Strategy B: Framebuffer-First

Use this if the product priority is GUI confidence.

Steps:

1. Configure only display and timer devices.
2. Run with `stream_display=true`.
3. Hash framebuffer frames.
4. Assert frame count and stable hashes.

Debugging tactics:

- Start with one deterministic frame before checking streams.
- Save expected hashes in harness JSON output, not full binary frames.
- Compare trace timestamps with frame stream timestamps.

Success criteria:

- Cockpit lane reports deterministic framebuffer hashes across repeated runs.

### Strategy C: Interaction-First

Use this if touch/input correctness is the priority.

Steps:

1. Configure display plus touch.
2. Inject one touch event.
3. Inspect device state and trace output.
4. Add multi-touch or repeated input only after the single event is stable.

Debugging tactics:

- Trace touch injection, firmware receive, and UI/device state separately.
- Assert event order before asserting rendered output.

Success criteria:

- Touch injection produces the expected device state and trace evidence.

## 6. Telematics Firmware And Host-Socket Lane

### Blocker

The host-networking engine edges were hardened, but there is no telematics ECU
firmware and no dogfood host TCP rig for periodic uploads/remote queries.

### Strategy A: Firmware Ping-Pong First

Use this to prove a minimal telematics ECU.

Steps:

1. Add `firmware/telematics_ecu`.
2. Send one deterministic host upload or query.
3. Trace `telematics_send`, `telematics_recv`, and byte counts.
4. Add one scenario without stressing partial writes.

Debugging tactics:

- Prove firmware task timing before debugging host sockets.
- Keep payloads tiny until delivery is reliable.

Success criteria:

- A telematics scenario sends and receives one host message deterministically.

### Strategy B: Host-Socket Stress Harness

Use this after minimal firmware works.

Steps:

1. Add `harness telematics`.
2. Start the costar server or microcar scenario under test.
3. Connect a host TCP client with small socket buffers.
4. Send bursts and assert response count.

Debugging tactics:

- Log partial-write boundaries and total bytes drained.
- Assert conservation: bytes written by host equal bytes observed by simulated
  device plus buffered remainder.
- Run repeats to catch poller re-arm regressions.

Success criteria:

- No dropped requests under burst traffic and small buffers.

### Strategy C: Trace/Data Reconciliation

Use this for final product confidence.

Steps:

1. Capture host-side request IDs.
2. Capture firmware trace request IDs.
3. Capture network device inspection state.
4. Reconcile all three in harness output.

Debugging tactics:

- If reconciliation fails, classify the failure as host-send, simulator-deliver,
  firmware-handle, or firmware-reply.
- Keep request IDs monotonic and small for readable traces.

Success criteria:

- Every request has matching host, simulator, and firmware evidence.

## 7. Per-Session State Ownership Refactor

### Blocker

Some simulator state is still process-global or thread-local across sessions.
Moving it is high risk because C FFI reads timing, task identity, and device
state during guest firmware execution.

### Strategy A: Device Registry First

Use this as the safest initial refactor.

Steps:

1. Identify all device registry reads/writes.
2. Move one device class to per-World ownership.
3. Keep an active-simulator guard only for C callbacks.
4. Add regression tests for two Worlds in one process.

Debugging tactics:

- Start with a low-risk device class before CAN/network/display.
- Assert no cross-World device leakage.
- Run golden microcar scenarios after each device class.

Success criteria:

- Two in-process Worlds can use the migrated device without shared state.

### Strategy B: Clock/Task Identity Audit

Use this before touching `SIM_NOW` or task IDs.

Steps:

1. List every read/write of `SIM_NOW`, current task ID, and scheduler identity.
2. Add assertions documenting the current active simulator.
3. Move reads behind a context object where possible.
4. Keep FFI behavior byte-identical.

Debugging tactics:

- Add temporary panic-on-missing-active-sim checks in tests, not production paths.
- Compare golden trace hashes before and after each step.
- Avoid simultaneous clock and device registry moves.

Success criteria:

- Existing timing traces remain stable.
- In-process multi-session tests stop sharing task/clock identity.

### Strategy C: Product Surface Driven

Use this if cockpit or telematics exposes concrete session leakage.

Steps:

1. Write a failing two-session test that reproduces the leak.
2. Fix only the state class involved.
3. Add the failing test as the regression.

Debugging tactics:

- Prefer concrete leakage tests over speculative refactors.
- Record which state crossed the session boundary.

Success criteria:

- The two-session repro passes and no dogfood lane regresses.

## 8. Remaining Breakpoint Predicates And Nightly/Scale

### Blocker

Message breakpoint exists. Other predicates need product events or more
firmware traces: machine, vehicle-state, device-state, DTC-creation, and
assertion-failure.

### Strategy A: Predicate Wrappers Around Existing Traces

Use this for vehicle mode and DTC now that diagnostics emits traces.

Steps:

1. Define trace labels accepted by each predicate.
2. Add wrappers over `continue_until`.
3. Add unit tests with synthetic traces.
4. Add one scenario-level dogfood check.

Debugging tactics:

- If a predicate never fires, verify the trace label exists before debugging
  `continue_until`.
- Keep predicate matching exact and documented.

Success criteria:

- Vehicle-state and DTC predicates work on diagnostics scenarios.

### Strategy B: Device-State Predicates After Cockpit

Use this when display/touch/device inspection exists in cockpit.

Steps:

1. Define device-state events or inspection snapshots.
2. Add predicate support for one device type first.
3. Expand after the first type is stable.

Debugging tactics:

- Do not infer device state from unrelated trace strings.
- Prefer structured inspection data where available.

Success criteria:

- A cockpit scenario can stop on a display/touch state condition.

### Strategy C: Nightly/Scale After Semantic Lanes

Use this after charging/OTA/telematics have small green lanes.

Steps:

1. Compose nightly from existing lane commands.
2. Add repeats and timeouts.
3. Add larger fleet/topology runs last.

Debugging tactics:

- Classify failures as nondeterminism, timeout, semantic assertion, or panic.
- Keep JSON reports for trend comparison.

Success criteria:

- A single nightly command reports lane totals and isolates failing scenarios.

## Recommended Execution Order

1. ~~Charging safety lane~~ — **done (M13)**: drive blocked while plugged.
2. Cockpit/gRPC, because `protoc` is unblocked and the lane is bounded.
3. OTA happy path, then OTA fault matrix.
4. Deeper charging FSM (handshake / temp-rise / reduced-current / complete) +
   charging plant physics; then flip the deferred `toml_zoo` `charging-while-drive`.
5. Diagnostics-over-bus or live BMS, depending on whether product-grade CAN
   assertions are required immediately.
6. Debug gym corpus expansion using diagnostics/charging first, OTA/telematics later.
7. Telematics firmware and host-socket lane.
8. Per-session state ownership, staged behind concrete two-session tests.
9. Nightly/scale once the semantic lanes are individually green.

## Fast Status Commands

From `/home/zmm/projects/microcar`:

```sh
cargo build --bin microcar
cargo test --manifest-path dogfood/Cargo.toml
MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- diagnostics
MICROCAR_BIN=target/debug/microcar cargo run --manifest-path dogfood/Cargo.toml --bin harness -- charging
```

From `/home/zmm/projects/costar`:

```sh
PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc cargo test -p sim-grpc
```
