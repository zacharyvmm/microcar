# costar + microcar Dogfood — Status & Blockers Report

_Branch: `dogfood-milestone-1` on both repos. Generated at Milestone 11._

- costar: `github.com/zacharyvmm/costar` @ `1ee4ad0`
- microcar: `github.com/zacharyvmm/microcar` @ `16174a9`
- Host: Linux, Rust 1.96.1, workspace at `/home/zmm/projects`.

This document explains **what is done**, **what remains**, and — in detail — **why each
remaining track is blocked** and exactly what input/decision is needed to unblock it.

---

## 1. What is complete (11 milestones, all pushed & verified)

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

**Fully-delivered plan tracks:** engine stabilization; networking/device-edge hardening;
`simfarm`; `toml_zoo`; `topology` (7/7); Trace v2 data model; debugging primitives
(`step`, `continue_until`, keyframe replay, message breakpoint) and the `debug_gym`
determinism invariants.

Test counts: costar `sim-world` 110 unit tests, `sim-core` 25; microcar `dogfood` 59 unit
tests. All lanes green: `harness topology` 7/7, `toml-zoo` 11/11, `simfarm` PASS,
`debug-gym` 4/4. All 29 non-soak vehicle scenarios pass with golden traces intact.

---

## 2. Remaining work and its blockers

Every remaining plan item is blocked on one of three things: **new firmware**, the
**`protoc` toolchain** (denied), or a **large/risky costar refactor** that needs sign-off.

### 2.1 Firmware EV lanes — `diagnostics`, `charging`, `ota`  ⟶ BLOCKED: needs new firmware + design decision

**What the plan wants:** a vehicle-mode state machine adding `CHARGING`, `SERVICE`,
`OTA_UPDATE` (and `TRANSPORT_MODE`), plus ECU firmware behavior:
- diagnostics: diagnostic session, read vehicle mode, read/clear DTCs, live BMS data,
  actuator self-test, "service mode disables drive".
- charging: plug/handshake/charging/temperature-rise/reduced-current/complete, "drive
  blocked while plugged".
- ota: download → write slot B → CRC → commit → reboot → health check → rollback, plus an
  8-case power-cut/corruption/reset fault matrix.

**Why it is blocked:**
1. These modes and behaviors **do not exist** in the microcar firmware today. Implementing
   them means writing new ECU firmware logic (the microcar `MicrocarFirmware` layer and
   possibly the C firmware sources) — a substantial authoring effort, not a bounded patch.
2. It is a **design decision**, not a mechanical one: how the vehicle-mode authority lives
   in the Gateway ECU, the DTC store, the UDS-like diagnostic request/response protocol,
   and the OTA slot/boot-flag model all need choices that should be confirmed against the
   product intent rather than invented unilaterally.
3. The two deferred `toml_zoo` cases (`charging-while-drive`, `ota-while-drive`) are
   waiting on these vehicle modes existing in the scenario schema — they can only be
   un-deferred once charging/OTA land.

**To unblock:** approve starting the firmware track and confirm the modeling approach
(depth of the vehicle-mode FSM, DTC/UDS shape, OTA slot model). Then it can be built lane
by lane (diagnostics first is the natural entry, since it introduces `SERVICE` mode + DTCs).

### 2.2 `debug_gym` seeded-bug corpus  ⟶ BLOCKED: depends on firmware (2.1)

**What the plan wants:** 7 seeded bugs (gateway race, powertrain timeout/cancel, BMS stale
sensor, dashboard missed warning, telematics partial-write, OTA rollback, gateway bridge
loop), each with: description, expected symptom, minimal failing scenario, **golden
failing trace**, required debugging primitive, and **fixed trace**.

**Why it is blocked:** the debug_gym *primitives* are done (M7/M8/M10/M11: step,
continue_until, keyframe replay, message breakpoint, and the determinism invariants). But
the *corpus* requires deliberately-buggy **firmware variants** to produce the golden
failing/fixed traces. That is the firmware work from 2.1. (The gateway-bridge-loop bug is
the one exception that is already exercised structurally by the topology
`gateway_loop_prevention` scenario.)

**To unblock:** same as 2.1 — needs firmware.

### 2.3 `cockpit` lane + gRPC control plane  ⟶ BLOCKED: `protoc` not installed (install attempt denied)

**What the plan wants:** dogfood the GUI-facing path — `CreateSession`, `ConfigureBoard`
(display + touch + timer + ADC), `Run(stream_display=true)`, `InjectTouch`,
`InspectDevices`, framebuffer-hash assertions, trace↔device reconciliation. This is the
`sim-grpc` product surface.

**Why it is blocked:** `sim-grpc`'s `build.rs` compiles `.proto` files via `tonic-build`,
which requires the **`protoc`** protobuf compiler. `protoc` is **not installed** on this
Linux host, so `cargo build -p sim-grpc` fails at the build-script step. This is a
pre-existing environment gap, unrelated to any change in this branch — every other crate
microcar depends on builds fine; `sim-grpc` is simply not in microcar's dependency tree,
which is why the dogfood work so far never needed it.

I attempted to install it (`apt-get install protobuf-compiler`); **the command was denied
(no consent granted)**, so I did not proceed and will not retry it without your go-ahead.

**To unblock (pick one):**
- Grant consent to run `sudo apt-get install -y protobuf-compiler` (or the distro
  equivalent), **or**
- Install `protoc` yourself, **or**
- Provide a vendored/`protoc-bin-vendored`-style path (but that would add a dependency,
  which the dogfood crate policy forbids; more natural to install the system tool).

Once `protoc` is present I can build `sim-grpc`, get its tests green, and start the cockpit
lane and the gRPC-dependent control-plane work.

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
4. It partly interlocks with the gRPC control-plane unification (2.3), which itself needs
   `protoc`.

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
  assertion-failure) are thin wrappers over the existing `continue_until` mechanism, but
  most only become **meaningful once firmware emits those events** (vehicle state, DTCs).
  The message breakpoint (`run_to_frame`) is done because CAN delivery already exists.

---

## 3. Summary of what input is needed

The autonomous track has been exhausted of **bounded, safe, no-new-dependency** increments.
The remaining work diverges into three directions that require a decision:

1. **Firmware EV lanes** (diagnostics → charging → ota) — the largest remaining *plan
   value*; needs approval to write firmware + confirm the modeling approach. Also unblocks
   the debug_gym seeded-bug corpus and the two deferred `toml_zoo` cases.
2. **`protoc` install** — one system-package install unblocks `sim-grpc`, the **cockpit**
   lane, and the gRPC side of the control-plane/per-session work. _(My install attempt was
   denied; needs your consent or a manual install.)_
3. **Per-session state refactor** — the other big costar track; high-risk, needs sign-off
   and a staged, heavily-verified approach.

Recommended order if all are eventually wanted: **(2) install `protoc`** (cheap, unblocks a
whole surface) → **(1) diagnostics firmware** (introduces `SERVICE` mode + DTCs, high plan
value, no `protoc` needed) → **(4/2.4) per-session state** last (riskiest).
