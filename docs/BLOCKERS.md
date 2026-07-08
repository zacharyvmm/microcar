# costar + microcar Dogfood — Status & Blockers Report

_Branch: `dogfood-milestone-1` on both repos. Updated after the M13 charging lane._

- costar: `github.com/zacharyvmm/costar` @ `1ee4ad0`
- microcar: `github.com/zacharyvmm/microcar` @ `6f2c0e8`
- Host: Linux, Rust 1.96.1, workspace at `/home/zmm/projects`.

This document explains **what is done**, **what remains**, and — in detail — **why each
remaining track is blocked** and exactly what input/decision is needed to unblock it.

---

## 1. What is complete (13 milestones, verified locally)

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

**Fully-delivered plan tracks:** engine stabilization; networking/device-edge hardening;
`simfarm`; `toml_zoo`; `topology` (7/7); Trace v2 data model; debugging primitives
(`step`, `continue_until`, keyframe replay, message breakpoint), the `debug_gym`
determinism invariants, the first diagnostics lane, and the first charging safety lane.

Test counts: costar `sim-world` 110 unit tests, `sim-core` 25; microcar `dogfood` 68 unit
tests. All lanes green: `harness topology` 7/7, `toml-zoo` 11/11, `simfarm` PASS,
`debug-gym` 4/4, `diagnostics` 2/2, `charging` 1/1. All 29 non-soak vehicle scenarios pass
with golden traces intact.

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
   reduced-current / charge-complete) and the charging plant physics. **OTA** behavior still
   does not exist in firmware at all and needs new ECU logic + scenario contracts.
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

### 2.2 `debug_gym` seeded-bug corpus  ⟶ BLOCKED: depends on firmware (2.1)

**What the plan wants:** 7 seeded bugs (gateway race, powertrain timeout/cancel, BMS stale
sensor, dashboard missed warning, telematics partial-write, OTA rollback, gateway bridge
loop), each with: description, expected symptom, minimal failing scenario, **golden
failing trace**, required debugging primitive, and **fixed trace**.

**Why it is blocked:** the debug_gym *primitives* are done (M7/M8/M10/M11: step,
continue_until, keyframe replay, message breakpoint, and the determinism invariants). But
the *corpus* requires deliberately-buggy **firmware variants** to produce the golden
failing/fixed traces. The new diagnostics dogfood variants can seed one or two cases, but
the full corpus still needs more firmware variants (especially telematics and OTA). The
gateway-bridge-loop bug is the one exception already exercised structurally by the topology
`gateway_loop_prevention` scenario.

**To unblock:** same as 2.1 — needs firmware.

### 2.3 `cockpit` lane + gRPC control plane  ⟶ UNBLOCKED LOCALLY: workspace `protoc` installed

**What the plan wants:** dogfood the GUI-facing path — `CreateSession`, `ConfigureBoard`
(display + touch + timer + ADC), `Run(stream_display=true)`, `InjectTouch`,
`InspectDevices`, framebuffer-hash assertions, trace↔device reconciliation. This is the
`sim-grpc` product surface.

**Previous blocker:** `sim-grpc`'s `build.rs` compiles `.proto` files via `tonic-build`,
which requires the `protoc` protobuf compiler. `protoc` was not installed system-wide, and
the earlier `apt-get install protobuf-compiler` path was not usable without root.

**Local unblock:** a workspace-local official protobuf compiler is installed at:

```text
/home/zmm/projects/.tools/protoc-27.3/bin/protoc
```

Use it by exporting `PROTOC` when building or testing costar:

```sh
cd /home/zmm/projects/costar
PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc cargo build -p sim-grpc
PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc cargo test -p sim-grpc
```

Verified: `PROTOC=/home/zmm/projects/.tools/protoc-27.3/bin/protoc cargo test -p sim-grpc`
passes all 14 `sim-grpc` integration tests when localhost binding is allowed. In Codex's
sandbox, those tests need an escalated run because they bind `127.0.0.1:0`.

**Next step:** start the `cockpit` lane against the now-buildable `sim-grpc` product
surface.

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

The autonomous track has cleared the bounded environment issue and delivered the first
charging safety lane (M13). Remaining work now diverges into two decision-heavy directions,
plus the newly unblocked cockpit lane:

1. **Remaining firmware EV lanes** — the charging *safety* lane (drive blocked while
   plugged) is done (M13); what remains needs approval + a confirmed modeling approach: the
   richer charging FSM (handshake / temperature-rise / reduced-current / charge-complete) and
   battery plant physics, **OTA** end-to-end (download → slot B → CRC → commit → reboot →
   health check → rollback + 8-case fault matrix), and diagnostics live-BMS. These also
   unblock most of the debug_gym seeded-bug corpus and the two deferred `toml_zoo` cases.
2. **Cockpit / gRPC lane** — no longer blocked by `protoc`; proceed with the local
   `PROTOC` path above.
3. **Per-session state refactor** — the other big costar track; high-risk, needs sign-off
   and a staged, heavily-verified approach.

Recommended order if all are eventually wanted: **cockpit / gRPC** (now unblocked and
cheap to start) → **OTA firmware** + the deeper charging FSM → **per-session state** last
(riskiest, but needed for product-grade in-process session/device isolation).
