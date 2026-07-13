# costar + microcar Dogfood Implementation Guide

Last reviewed: 2026-07-13

## 1. Objective

Finish the compact-EV dogfood project so `microcar` proves the following
`costar` product behavior end to end:

- isolated in-process Worlds and gRPC sessions;
- receiver-correct CAN and isolated virtual devices;
- restartable firmware with persistent flash and volatile-state reset;
- protocol-driven diagnostics, charging, and OTA;
- a real dashboard display/touch path;
- deterministic telematics over virtual Ethernet and a host TCP bridge;
- seven reproducible debug-gym bugs and typed debugging predicates;
- deterministic CI coverage at normal, concurrent, and fleet scale.

Implement the stages below in order. Later stages may rely only on acceptance
tests from earlier stages, not on trace labels or synthetic state mutation.

## 2. Repository Boundary

Use these ownership rules throughout:

| Concern | Repository and location |
|---|---|
| execution context, device/network ownership, CAN routing, restart, gRPC, breakpoints | `costar/crates/*` |
| CAN payloads, ECU state machines, firmware, vehicle rules | `microcar/common` and `microcar/firmware` |
| SOC, temperature, speed, charge/discharge behavior | `microcar/plant` |
| automotive scenario validation | `microcar/src/validate.rs` |
| scenarios and semantic lane assertions | `microcar/dogfood` |

Do not put vehicle modes, BMS fields, OTA rules, or charging rules into `costar`.
Do not implement generic device/session ownership in `microcar`.

## 3. Completion Rules

A lane is complete only when its producer, transport/device, consumer, and
observable result execute independently. The harness must assert semantic state
and transport/device evidence. A firmware-local `sim_trace_*` call is supporting
evidence, not the behavior under test.

All additions must preserve existing human golden traces. New behavior must be
enabled by a new scenario, new firmware selection, or a backward-compatible API
field. Keep the existing trace-backed diagnostics, charging, and OTA fixtures as
regressions until their real replacements pass 100 repeated runs.

## 4. Existing Foundation to Keep

The repository already contains:

- engine correctness fixes, the dogfood harness, `simfarm`, and `toml_zoo`;
- topology 7/7, Trace v2, gateway forwarding, and correlation causality;
- stepping, `continue_until`, message breakpoints, keyframes, and replay;
- four debug-gym corpus pairs;
- initial trace-backed diagnostics/charging/OTA lanes;
- a sequential gRPC cockpit test with an empty framebuffer;
- retained-owner `DeviceBank`/`SimGlobal` activation stacks;
- opt-in machine-owned device banks and an owned-bank CAN execution path;
- firmware factories, reboot downtime, stopped-machine frame dropping, and a
  microcar gateway-reboot scenario.

Do not reimplement these primitives. Extend them and add the missing product-path
acceptance described below.

## 5. Stage A — Finish World, Device, gRPC, and Restart Ownership

This stage is the gate for every later stage.

### A1. Add explicit machine-device access to `sim-world`

Modify:

- `costar/crates/sim-world/src/machine.rs`
- `costar/crates/sim-world/src/world.rs`
- `costar/crates/sim-world/src/board.rs`

Add these public methods:

```rust
impl Machine {
    pub fn with_device_context<R>(&self, f: impl FnOnce() -> R) -> R;
}

impl World {
    pub fn with_machine_devices<R>(
        &self,
        machine_id: u64,
        f: impl FnOnce() -> R,
    ) -> Result<R, WorldError>;

    pub fn machine_ids(&self) -> impl Iterator<Item = u64> + '_;
}
```

`Machine::with_device_context` must call the existing retained-owner
`SimulatorExecutionContext::with_active`. `World::with_machine_devices` must
resolve exactly one machine and execute the closure in that machine's context.
It must never fall back to the most recently active machine.

Add `WorldError::MachineNotFound(u64)` and return it for a missing target. Do not
expose `DeviceBank` itself through the public API.

Store the board definition on `Machine` as a cloneable `BoardConfig`. Extend
`board::PeripheralDef` with `Option<u16>` display width/height,
`Option<String>` color mode, and `Option<u32>` touch display ID fields
corresponding to gRPC `PeripheralDef`. Accept only `rgb565`, `rgb888`, and
`argb8888`. Defaults are 320×240 RGB565; touch without a display ID targets
display 0. Add:

```rust
impl Machine {
    pub fn configure_board(&mut self, board: BoardConfig) -> Result<usize, BoardError>;
    pub fn board_config(&self) -> &BoardConfig;
}
```

`configure_board` replaces the complete board definition, validates it, and
initializes it inside `with_device_context`. Partial/append configuration is not
supported. gRPC must translate its `PeripheralDef` list into this `BoardConfig`
and call this method instead of duplicating the device-construction match.

Make `SessionMap::load_scenario`, clone, reset, and keyframe rebuild call
`world.enable_owned_device_banks()` before any board configuration or firmware
attachment. Keep the legacy no-owned-bank path only for existing direct
single-simulator compatibility tests.

### A2. Make gRPC targeting explicit and backward compatible

Modify:

- `costar/crates/sim-grpc/proto/simulator.proto`
- `costar/crates/sim-grpc/src/session.rs`
- `costar/crates/sim-grpc/src/server.rs`
- `costar/crates/sim-grpc/tests/cockpit_test.rs`

Add these protobuf fields using the stated field numbers:

```proto
message ConfigureBoardRequest {
  uint64 session_id = 1;
  repeated PeripheralDef peripherals = 2;
  optional uint64 machine_id = 3;
}

message InspectDevicesRequest {
  uint64 session_id = 1;
  string device_type = 2;
  uint32 device_id = 3;
  optional uint64 machine_id = 4;
}

message TouchInject {
  uint32 device_id = 1;
  repeated TouchEvent events = 2;
  optional uint64 machine_id = 3;
}

message DisplayFrame {
  uint32 device_id = 1;
  uint32 width = 2;
  uint32 height = 3;
  string color_mode = 4;
  repeated DirtyRect dirty_rects = 5;
  bool full_frame = 6;
  uint64 machine_id = 7;
}
```

Target resolution is fixed:

1. If `machine_id` is present, require that machine in the session World.
2. If absent and the World has exactly one machine, select it for compatibility.
3. If absent and the World has zero or multiple machines, return
   `INVALID_ARGUMENT`; never choose the first machine.

Change `SessionMap` from one global `Mutex<HashMap<u64, Session>>` to:

```rust
Mutex<BTreeMap<u64, Arc<Mutex<Session>>>>
```

The map lock is used only to look up/insert/remove the `Arc`. Operations then
lock one session. `list()` iterates the `BTreeMap`, giving deterministic order.
Retain the existing take/return World design in the run worker. Every exit
path—success, stop, client disconnect, simulation error, or panic—must return
the World or mark the session `Error`. Wrap the worker body in
`catch_unwind(AssertUnwindSafe(...))`.

Use these exact terminal states: natural completion and explicit Stop → `Done`;
client disconnect → `Paused`; simulation error or panic → `Error`. While the
World is checked out, setup, inspection, keyframe, clone, and reset operations
return `FAILED_PRECONDITION` with `session is running`; streamed touch/pause/
resume/stop commands remain valid.

Replace all direct device-registry calls in `configure_board`, `inspect_devices`,
touch handling, and display streaming with `World::with_machine_devices`.
Display streaming iterates `(machine_id, device_id)` and sets
`DisplayFrame.machine_id`. Touch commands carry their target through
`ClientCommand`.

### A3. Preserve persistent devices across restart

Modify:

- `costar/crates/sim-devices/src/bank.rs`
- `costar/crates/sim-world/src/machine.rs`
- `costar/crates/sim-world/src/world.rs`

Add a cloneable `PersistentDeviceState` containing the selected machine's
`VirtualFlash`, `VirtualEeprom`, and virtual block devices. Add:

```rust
impl DeviceBank {
    pub fn snapshot_persistent(&self) -> PersistentDeviceState;
    pub fn restore_persistent(&self, state: PersistentDeviceState);
    pub fn reset_volatile(&self);
}
```

`reset_volatile` clears/recreates CAN, UART, timers, IRQ state, framebuffer and
display dirty state, touch queues, ADC transient state, and fault-injector state,
but does not modify flash, EEPROM, or block contents. Network state is handled
separately in Stage B3.

The restart algorithm in `World` must be exactly:

1. Emit `machine_reset_begin` at the fault time.
2. Snapshot persistent devices and immutable machine specification: ID, name,
   RTOS, `SimConfig`, firmware factory, and board configuration.
3. Remove the old machine, its receiver inbox, and pending device input.
4. Mark the machine stopped until `boot_at = now + downtime_ms * 1000`.
5. Drop bus deliveries whose receiver is stopped. Frames already transmitted to
   other receivers continue normally.
6. At `boot_at`, reconstruct the machine from the immutable specification,
   enable its owned bank, recreate its board, restore persistent devices, attach
   firmware from the factory, mark it running, then emit `machine_reset_boot`.
7. Process bus arrivals at `boot_at` only after the boot sequence. Arrivals with
   time `< boot_at` are dropped; arrivals with time `== boot_at` are post-boot.

Do not preserve guest RAM, fibers, C instance state, volatile device queues, or
the pre-reset CAN inbox.

### A4. Required acceptance tests

Add tests with these exact responsibilities:

- `sim-world`: `two_worlds_owned_can_interleave_100x`—two actual Worlds on one
  thread, both using controller 0, run in A/B and B/A order; each trace equals
  its solo trace and contains only its frame IDs.
- `sim-world`: `owned_can_drains_tx_from_only_firmware_step`—a frame emitted by
  firmware at the former `advance_to` boundary is delivered before the World is
  done and is attributed to the sender's attached bus only.
- `sim-world`: `restart_preserves_persistent_and_resets_volatile`—write flash,
  enqueue CAN/touch state, reboot, then assert flash survives and queues do not.
- `sim-world`: `restart_downtime_delivery_boundary`—one frame before boot is
  absent and one at boot is received exactly once.
- `sim-grpc`: `concurrent_sessions_isolate_device_zero`—two sessions configure
  display/touch/timer/ADC/CAN ID 0 on different machines, inject distinct touch
  events, run concurrently, and inspect/stream only their own values.
- `sim-grpc`: `failed_session_returns_world_and_sibling_runs`—one run panics or
  errors while a sibling completes and remains inspectable.
- `microcar`: extend `dogfood/b3_gateway_reboot_downtime.toml` so it asserts a
  second gateway boot, heartbeat resumption after downtime, preserved RTOS, and
  uninterrupted sibling heartbeat—not merely reset markers.

### A5. Unify run semantics and bound server state

Add `costar/crates/sim-world/src/control.rs` with one runner used by JSON-RPC and
gRPC:

```rust
pub enum RunLimit { ToCompletion, Until(Tick), EventCount(u64) }
pub enum RunTermination { Completed, LimitReached, Paused, Stopped, Error, Panic }
pub struct RunOutcome { pub termination: RunTermination, pub now: Tick, pub events: u64, pub error: Option<String> }
pub fn drive_world(world: &mut World, limit: RunLimit) -> RunOutcome;
```

`drive_world` is the only control-plane loop around `World::step`; `World::run`
and `run_until` remain engine APIs. It catches panics at the guest boundary and
never advances past `Until`. JSON-RPC `sim.run`, `sim.run_until`, and `sim.step`
and gRPC Run call this function.

Refactor the JSON-RPC session map in `sim-runner/src/serve.rs` to the same
`BTreeMap<u64, Arc<Mutex<Session>>>` and take/return pattern specified for gRPC,
so neither server holds its global map lock during simulation.

Apply these fixed resource limits to both servers:

- 128 live sessions; creation beyond the limit returns resource exhausted;
- 16 keyframes per session; saving the 17th evicts the oldest;
- 100,000 retained trace records per session using a ring buffer and a
  `dropped_trace_records` counter;
- 300 seconds idle TTL for Idle, Ready, Done, and Error sessions; Running and
  Paused sessions are not TTL-expired;
- cleanup check at most once per 30 host seconds and on every create/list call.

Add tests for deterministic listing, session-limit rejection, keyframe eviction,
trace-ring eviction/counter, TTL cleanup, and identical JSON-RPC/gRPC outcomes
for completion, deadline, error, and panic cases.

Stage A is complete only when the two-World interleave and concurrent-session
tests each pass in a 100-iteration loop, all other A4 tests pass, and the A5
control-plane/lifecycle tests pass.

## 6. Stage B — Isolate Time, Task Identity, C Firmware State, and Networking

Complete this before claiming real-firmware concurrent sessions or duplicate ECU
instances.

### B1. Extend the active execution context

Modify:

- `costar/crates/sim-ffi/src/lib.rs`
- `costar/crates/sim-ffi/src/simulator.rs`
- `costar/crates/sim-ffi/include/sim_abi.h`

Create one retained `GuestRuntime` per `Simulator`:

```rust
pub struct GuestRuntime {
    now: Cell<Tick>,
    current_task_id: Cell<u64>,
    instance_regions: RefCell<BTreeMap<u32, AlignedRegion>>,
}
```

Add it to `SimulatorExecutionContext` beside `SimGlobal` and `DeviceBank`.
Activation uses the same retained-owner stack pattern already used for those
objects. Replace reads/writes of process/thread-global `SIM_NOW` and
`CURRENT_TASK_ID` with the active `GuestRuntime`. Calls with no active context
may use the existing fallback only for legacy unit tests.

Add this C ABI:

```c
void *sim_instance_state(uint32_t key, uint32_t size, uint32_t alignment);
```

Rules:

- the first call allocates zeroed, correctly aligned storage owned by the active
  machine;
- later calls with the same key must use identical size/alignment or fail with a
  null return and a traceable error;
- storage has a stable address for the lifetime of that machine runtime;
- restart drops all regions;
- no active machine returns null;
- implement `AlignedRegion` with `std::alloc::Layout`, allocation, zeroing, and a
  correct `Drop`; do not rely on `Vec<u8>` alignment.

### B2. Migrate mutable C globals

For each ECU create one context struct in its `main.c` and retrieve it through
`sim_instance_state` using fixed keys:

| Key | ECU |
|---:|---|
| `0x4D430001` | gateway |
| `0x4D430002` | powertrain |
| `0x4D430003` | BMS |
| `0x4D430004` | dashboard FreeRTOS |
| `0x4D430005` | dashboard Zephyr |
| `0x4D430006` | diagnostics tool |
| `0x4D430007` | OTA tool |
| `0x4D430008` | telematics |

Move every mutable file/function static into the context, including task
handles, queues, semaphores, timers, stacks, TCBs, bug flags, script cursors,
and OTA state. Immutable `static const` lookup data may remain global. Task entry
functions call the ECU context accessor rather than retaining a pointer to a
different machine's context.

Migrate in this order: gateway, powertrain, BMS, dashboards, diagnostics, then
new OTA/telematics firmware. Delete comments claiming one instance per process.

### B3. Scope networking per machine

Refactor `sim-net` registries into a cloneable `NetworkBank` owned by the
`SimulatorExecutionContext`, using the same activation model as `DeviceBank`.
It owns Ethernet devices, TCP framing buffers, pollers, TAP handles, and cleanup
state. World packet injection and TX draining must activate the sender/receiver
machine context. Destroying a session drops its host handles and poller entries.

Add tests for two Ethernet device-0 instances, two fragmented TCP streams, and
session destruction/recreation without stale readiness events.

### B4. Isolation acceptance

- Two gateway instances in one World have independent modes, DTCs, tasks, and
  task IDs.
- Two BMS instances receive different sensor frames and publish different
  limits without cross-observation.
- Two concurrent real-firmware gRPC sessions reproduce their solo Trace v2
  hashes.
- Reboot resets the selected ECU's C context but not its sibling's context.
- Two network device-0 instances conserve their own frames under interleaving.

## 7. Stage C — Define Protocol-Backed External Actors

Do not add a scenario `mode` field. Vehicle state remains gateway-owned.

### C1. Protocol additions

Modify `microcar/common/include/microcar_protocol.h`, its documentation, and any
Rust mirror. Reserve these node IDs:

```c
#define MC_NODE_EVSE       6
#define MC_NODE_OTA_TOOL   7
#define MC_NODE_TELEMATICS 8
```

Reserve these CAN IDs and exact packed payloads:

| ID | Name | Payload bytes |
|---:|---|---|
| `0x203` | `BMS_CHARGE_LIMIT` | `[source=3, max_current_a_x2, soc_percent, temp_c_x10_le16, fault, seq]` (7) |
| `0x610` | `EVSE_EVENT` | `[source=6, event, request_id, offered_current_a_x2, target_soc, reserved]` (6) |
| `0x611` | `CHARGE_COMMAND` | `[source=1, state, request_id, current_a_x2, target_soc, reason]` (6) |
| `0x630` | `OTA_REQUEST` | `[source=7, request_id, image_id, total_chunks]` (4) |
| `0x631` | `OTA_CHUNK` | `[source=7, request_id, chunk_index, data0..data4]` (8) |
| `0x632` | `OTA_FINISH` | `[source=7, request_id, total_chunks, crc32_le]` (7) |
| `0x633` | `OTA_STATUS` | `[source=1, request_id, state, status, active_slot, target_slot, reason, seq]` (8) |

Extend the existing `BMS_STATUS` (`0x200`) payload from 7 to 8 bytes by adding
`seq` at byte 7. Bytes 0..6 retain their current encoding, so existing consumers
that read only those bytes remain compatible.

EVSE event values are `0=UNPLUG`, `1=PLUG`, `2=HANDSHAKE_OK`, `3=STOP`.
Charging state values are `0=DISCONNECTED`, `1=PLUG_DETECTED`, `2=HANDSHAKE`,
`3=ACTIVE`, `4=LIMITED`, `5=COMPLETE`, `6=FAULT`.

Add `MC_DIAG_STALE = 3`. For `MC_DIAG_LIVE_BMS`, request `param` selectors are:

- `0`: response `value0=soc_percent`, `value1=temp_c + 40`;
- `1`: response values are pack voltage in 100 mV, little-endian `u16`;
- `2`: response values are pack current in 100 mA, little-endian `i16`.

### C2. Scenario actors and validation

External actors are passive machines with no firmware, attached to a bus, and
send through existing `[[bus_inject]]`. Use names `evse`, `ota_tool`, and
`test_harness`.

In `microcar/src/validate.rs`, for IDs `0x610` and `0x630..0x632`, require:

- sender exists and has no firmware;
- sender is attached to the named bus;
- payload byte 0 equals the reserved node ID for that sender;
- payload length and enum ranges match the table above;
- request IDs are nonzero;
- OTA chunk indexes begin at 0, increase by 1, and do not exceed
  `total_chunks - 1`;
- `OTA_FINISH.total_chunks` equals both `OTA_REQUEST.total_chunks` and the number
  of observed chunks;
- no handshake precedes plug and no OTA chunk precedes an `OTA_REQUEST` event.
  Runtime gateway admission, not static validation, determines whether chunks
  following a rejected request are ignored.

Add `dogfood/charging/charging_while_drive.toml` and
`dogfood/ota/ota_while_drive.toml` as valid hostile semantic scenarios, not
malformed parser inputs. `harness charging` asserts the former rejection and
`harness ota` asserts the latter.

## 8. Stage D — Real Diagnostics over CAN

Modify:

- `microcar/firmware/bms_ecu/src/main.c`
- `microcar/firmware/gateway_ecu/src/main.c`
- `microcar/firmware/diagnostics_tool_ecu/src/main.c`
- `microcar/dogfood/src/diagnostics.rs`

The BMS already consumes plant sensor frame `0x500`. After every valid sensor
update it increments an 8-bit wrapping sequence and publishes `BMS_STATUS`
(`0x200`) with that sequence in byte 7. The gateway caches the complete status,
sequence, and receive time. A snapshot is fresh for exactly 500 ms
(`age_ms <= 500`).

On `MC_DIAG_LIVE_BMS`, the gateway returns the selector encoding from Stage C.
If no snapshot exists or its age is greater than 500 ms, return
`MC_DIAG_STALE` and zero values. The diagnostics tool sends selectors 0, 1, and
2 with distinct request IDs and assembles one decoded snapshot.

Add scenarios:

- `dogfood/diagnostics/live_bms_data.toml`—plant → BMS → gateway → tool;
- `dogfood/diagnostics/stale_bms_data.toml`—stop BMS publication, wait 501 ms,
  query, receive `STALE`;
- `dogfood/diagnostics/actuator_test_rejected_in_drive.toml`.

For each response, the harness must find Trace v2 edges for tool TX, gateway RX,
gateway TX, and tool RX. Tool TX and gateway RX share one correlation ID;
gateway TX and tool RX share a second correlation ID; the protocol request ID
connects the request and response legs. It must also assert decoded values equal
the plant values with the documented scaling. This stage replaces trace-script
diagnostics as the authoritative lane.

## 9. Stage E — Closed-Loop Charging

### E1. Pure FSM

Add:

- `microcar/common/include/microcar_charging.h`
- `microcar/common/src/microcar_charging.c`
- a Rust mirror in `microcar/state_tests/src/charging.rs`

The pure transition function takes current state plus one EVSE/BMS event and
returns `{next_state, command_current_a_x2, reject_reason}`. Implement only these
transitions:

```text
DISCONNECTED + PLUG                 -> PLUG_DETECTED
PLUG_DETECTED + HANDSHAKE_OK        -> HANDSHAKE
HANDSHAKE + fresh BMS limit > 0     -> ACTIVE
ACTIVE + lower nonzero BMS limit    -> LIMITED
ACTIVE/LIMITED + soc >= target_soc  -> COMPLETE
any plugged state + critical fault  -> FAULT
any plugged state + UNPLUG          -> DISCONNECTED
```

All other combinations leave state unchanged and return a nonzero rejection
reason. Vehicle mode is `CHARGING` from `PLUG_DETECTED` through `COMPLETE`.
Charging-state `FAULT` causes vehicle mode `FAULT`. Unplug then returns the
charging FSM to `DISCONNECTED`; the normal gateway fault-recovery rules decide
the next vehicle mode.

Current command is `min(EVSE offered current, BMS max current)`. Use units of
0.5 A. BMS limits are: 64 (32 A) below 45.0°C, 32 (16 A) from 45.0°C through
59.9°C, and 0 at 60.0°C or a critical fault. Target SOC is the EVSE payload,
clamped to 50..100.

### E2. Plant and firmware path

Extend `microcar/plant/src/battery.rs` with:

```rust
pub fn step_with_current(&mut self, current_ma: i32, dt_ms: u32);
```

Positive current discharges; negative current charges. Use the existing 50 Ah
capacity calculation for both directions, clamp SOC to 0..100%, and retain I²R
heating/cooling. `current_ma` storage becomes `i32`; encode saturated `i16` in
the existing CAN payload.

Extend `EnvironmentModel`/World with a deterministic plant CAN inbox. The plant
must consume `MOTOR_COMMAND` and `CHARGE_COMMAND` frames delivered through the
bus; remove the MVP behavior that maps driver throttle directly to motor torque
once a real command has been received. Do not let firmware call the plant
directly.

Gateway owns the charging FSM, BMS publishes `BMS_CHARGE_LIMIT`, powertrain
clamps torque independently whenever its received vehicle mode is not DRIVE or
LIMP, and the plant applies the received charge current.

Add scenarios:

- `plug_handshake_active.toml`;
- `high_temperature_limited.toml`;
- `charge_complete.toml`;
- `bms_fault_stops_charging.toml`;
- `drive_while_plugged.toml`.

Harness assertions: exact FSM sequence, real CAN deliveries, current never above
both limits, SOC increases, temperature follows the fixed model, fault commands
zero current, and drive while plugged produces zero torque.

## 10. Stage F — OTA through CAN, Flash, and Restart

### F1. Persistent layout

Use gateway flash device 0 with the default 256-byte pages:

| Pages | Purpose |
|---|---|
| 0 | metadata copy A |
| 1 | metadata copy B |
| 2-31 | slot A image |
| 32-61 | slot B image |
| 62-63 | reserved |

Metadata is this exact little-endian packed 32-byte record:

```text
0..3   magic = 0x4D434F54
4      format_version = 1
5      active_slot
6      target_slot
7      state
8      committed
9      boot_attempt_count
10     healthy
11     abort_reason
12..15 generation u32
16..19 image_length u32
20..23 image_crc32 u32
24..27 reserved = 0
28..31 record_crc32 over bytes 0..27
```

On update, erase and write the metadata page with the lower generation using
`generation+1`, read it back and validate its record CRC, then erase the old
metadata page. On boot, choose the valid record with the highest generation; if
neither is valid, initialize slot A as known-good.

Extend `microcar_ota_slot` with `abort(reason)` and `recover_after_reset(record)`.
Mirror every transition in `state_tests/src/ota_slot.rs`.

### F2. OTA tool and gateway worker

Add `microcar/firmware/ota_tool_ecu/src/main.c` and wire its boot function in
`build.rs`, `src/lib.rs`, and ECU resolution. The tool protocol is Stage C's
`0x630..0x633`. Test images use five-byte chunks. CRC32 is the reflected IEEE
algorithm with polynomial `0xEDB88320`, initial value `0xFFFFFFFF`, and final
XOR `0xFFFFFFFF`.

Gateway admission requires all of:

- received vehicle mode is not DRIVE;
- charging state is DISCONNECTED;
- no critical BMS fault;
- BMS status age is at most 500 ms;
- no update is already active.

After admission, gateway enters `OTA_UPDATE`, broadcasts vehicle mode, erases
the inactive slot pages, writes chunks in order, verifies count and CRC, writes
commit metadata, and requests a generic reboot with 10 ms downtime. Boot
recovery reads flash metadata. A committed target receives one health attempt;
success marks it active/healthy, failure writes rollback metadata and reboots
slot A. Powertrain independently clamps torque throughout.

### F3. Exact scenarios

Add one real tool-driven happy path plus all eight fault/admission cases:

1. `happy_path.toml`;
2. `corrupt_image.toml`;
3. `interrupted_write.toml`;
4. `power_cut_before_commit.toml`;
5. `failed_health.toml`;
6. `gateway_reset_during_update.toml`;
7. `bms_fault_during_update.toml`;
8. `ota_rejected_drive.toml`;
9. `ota_rejected_charging.toml`.

The OTA harness must inspect flash metadata before and after reboot, assert the
selected boot slot, and verify CAN request/status causality. The old five
trace-backed fixtures remain regressions but do not count toward these cases.

## 11. Stage G — Real Dashboard and Cockpit

Use the existing virtual display and touch devices on dashboard machine ID 4,
device ID 0. Implement a renderer shared by FreeRTOS and Zephyr dashboards.

Use a 320×240 RGB565 little-endian framebuffer. Clear the full framebuffer on a
screen-state change and use these exact colors and regions:

| State | Background RGB565 | Required regions |
|---|---|---|
| boot | `0x0000` | white `0xFFFF` rectangle `(40,108,240,24)` |
| READY | `0x0010` | green `0x07E0` status `(0,0,320,40)`, speed region `(20,70,120,80)` |
| DRIVE | `0x0200` | green status `(0,0,320,40)`, speed `(20,70,120,80)`, torque `(180,70,120,80)` |
| LIMP | `0xFD20` | amber status `(0,0,320,40)`, speed and torque regions as DRIVE |
| FAULT | `0x7800` | red `0xF800` warning `(0,170,320,70)` |
| CHARGING | `0x4010` | SOC bar `(20,90,280,24)`, current bar `(20,140,280,24)` |
| OTA_UPDATE | `0x0008` | progress bar `(20,110,280,24)` |

Region borders are white and one pixel wide. Bar interiors use green and fill
left-to-right: `filled_width = inner_width * value / maximum`, integer floor.
Speed and torque use white seven-segment digits with 8-pixel stroke width in
their regions; unused pixels retain the background. OTA progress is derived
only from OTA state: IDLE 0%, DOWNLOADING 25%, VERIFYING 50%, COMMIT_PENDING 75%,
REBOOTING 90%, HEALTHY 100%, ROLLED_BACK 0%.

Do not add fonts. Draw fixed rectangles and seven-segment digits so output is
platform-independent. Render only on state change or every 100 ms. Mark only
changed rectangles dirty.

Each touch release within `(280..319, 0..39)` toggles page 0/1; press and move do
not toggle. No touch acknowledges warnings or changes gateway mode, DTCs,
charging, or OTA state.

Update the gRPC cockpit test to run two sessions concurrently with different
mode sequences. Assert:

- nonempty frames;
- `DisplayFrame.machine_id == 4`;
- known FNV-1a 64-bit hashes of the full row-major framebuffer byte sequence for
  every screen (store the constants in the test after the renderer is fixed);
- dirty rectangles are identical across repeats;
- inspected display/touch state agrees with the frame at the same virtual time;
- touch changes only the dashboard page;
- sessions never cross-observe pixels or touch events.

Add `harness cockpit` only after the gRPC integration test is green.

## 12. Stage H — Telematics

Add `microcar/firmware/telematics_ecu/src/main.c` and use network device 0.
Application records are big-endian length-prefixed:

```text
u16 payload_length | u8 type | u32 request_id | payload
```

Types are `1=STATUS`, `2=DIAG_QUERY`, `3=LOCK`, `4=UNLOCK`,
`5=PRECONDITION`, `6=FAULT_UPLOAD`, `0x80=ACK`, `0x81=ERROR`.
Request IDs start at 1 and increase monotonically. Payloads after the common
type/request-ID header are:

- STATUS: `[vehicle_mode, soc_percent, temp_c_x10_le16, fault_code]`;
- DIAG_QUERY: `[selector]`, using the diagnostics selectors from Stage C;
- LOCK and UNLOCK: empty;
- PRECONDITION: `[target_temp_c_i8]`;
- FAULT_UPLOAD: `[source_node, fault_code, severity]`;
- ACK: `[original_type, status=0, response_data...]`;
- ERROR: `[original_type, error_code]`.

Every request receives exactly one ACK or ERROR with the same ID. The parser
retains incomplete header or payload bytes and may consume multiple complete
records from one read. Reject `payload_length < 5` and lengths greater than 256
with ERROR code 1; unknown type is ERROR code 2; invalid payload is code 3.

Implement two tests:

1. Virtual Ethernet: periodic status every 1,000 ms plus all remote commands;
   assert deterministic IDs, payloads, and one response per request.
2. Host loopback TCP: ephemeral port, 256-byte socket buffers, every request
   split at each possible byte boundary, then a 100-record burst. Use a 10-second
   wall timeout. Assert byte conservation, one response per ID, repeated poller
   wakeups, and complete socket/poller cleanup after session destruction.

Add `dogfood/src/telematics.rs`, `harness telematics`, and JSON summary output.

## 13. Stage I — Remaining Debug Seeds and Typed Breakpoints

### I1. Debug-gym corpus

Add paired `failing.toml`/`fixed.toml` directories using the real paths above:

- `bms_stale_sensor_bug`: buggy gateway uses snapshot without the 500 ms check;
- `dashboard_missed_warning_bug`: buggy renderer lets a normal mode repaint
  overwrite a critical warning;
- `telematics_partial_write_bug`: buggy parser discards an incomplete record.

Each pair must satisfy `bug-reproduced`, `bug-fixed`, and `traces-diverge`.
The localizing evidence must include snapshot age/sequence, dashboard warning
plus framebuffer hash, or request ID plus fragment boundary respectively.

### I2. Typed predicates

Keep `costar` generic. Add these structured predicate variants to `sim-world`:

```rust
enum ContinuePredicate {
    Semantic {
        machine_id: Option<u64>,
        event_type: String,
        fields: BTreeMap<String, ScalarValue>,
    },
    Device {
        machine_id: u64,
        device_type: DeviceType,
        device_id: u32,
        condition: DeviceCondition,
    },
    DroppedFrame { bus: String, message_id: u32 },
    AssertionFailure { name: String },
}

enum DeviceCondition {
    DisplayEnabled(bool),
    DisplayBacklight(u8),
    TouchPending(u32),
    CanRxQueueLen(u32),
    CanTxQueueLen(u32),
}
```

`ScalarValue` has only `Bool`, `U64`, `I64`, and `String`. Microcar emits semantic
events named `vehicle_state`, `dtc_created`, `bms_snapshot`, `ota_transition`,
and `dashboard_state`; their automotive field names remain in microcar code and
tests. A vehicle-state breakpoint is a `Semantic` match on `event_type =
"vehicle_state"` plus field `mode`; DTC creation matches `dtc_created` plus
`source` and `code`.

Evaluate semantic predicates over structured Trace v2 fields and device
predicates over session-owned snapshots, never formatted human strings. Field
matching is exact equality and all supplied fields must match. Reuse
`continue_until`; do not create a second scheduler loop. Every predicate family
needs a unit hit, unit miss, end-to-end scenario hit, and
keyframe/replay-equivalence test.

## 14. Stage J — Move Soak Tests to the Rust Simulator

The legacy Python runner must never execute `long_drive_10min.toml`,
`soak_1hour.toml`, or `overnight_8hour.toml` in the default suite. It advances in
10 ms increments, accumulates the complete JSONL trace, and then duplicates that
trace in `check_assertions.py`. The Rust lane below replaces it; do not optimize
the Python simulator for soak use.

### J1. Add bounded trace retention and online statistics

Modify:

- `costar/crates/sim-core/src/trace.rs`
- `costar/crates/sim-world/src/world.rs`
- `microcar/src/main.rs`

Add an opt-in trace-retention limit. Default behavior remains unbounded so
existing golden traces are byte-identical. Soak mode sets the limit to 4,096
records and evicts the oldest record when full.

Before retention/eviction, update a `TraceStats` accumulator containing:

```rust
pub struct TraceStats {
    pub event_count: u64,
    pub normalized_fnv1a64: u64,
    pub first_virtual_time: Option<u64>,
    pub last_virtual_time: Option<u64>,
    pub time_regressions: u64,
    pub can_tx_by_id: BTreeMap<u32, u64>,
    pub can_rx_by_id: BTreeMap<u32, u64>,
    pub dropped_by_id: BTreeMap<u32, u64>,
    pub assertion_failures: u64,
    pub retained_records: usize,
    pub evicted_records: u64,
}
```

Hash the same normalized event representation used by
`dogfood/src/trace_hash.rs`, before eviction. Time monotonicity is checked per
`(machine_id, source)` Trace v2 stream, not across concatenated human trace
sinks. CAN conservation allows only drops requested by a scenario fault.

Add these `microcar` CLI flags:

```text
--soak-summary-json <path-or->
--trace-retention <record-count>
```

`--soak-summary-json` suppresses human trace output and writes exactly one JSON
object to the path, or stdout for `-`. It contains scenario name, virtual ticks,
wall milliseconds, terminal status, every `TraceStats` field, final vehicle
mode, final SOC/temperature/speed, and scenario assertion results. It exits 0
only when simulation and assertions pass.

### J2. Add a bounded-memory Rust soak harness

Modify/add:

- `microcar/dogfood/src/runner.rs`
- `microcar/dogfood/src/soak.rs`
- `microcar/dogfood/src/lib.rs`
- `microcar/dogfood/src/bin/harness.rs`

Add `harness soak` with this interface:

```text
harness soak [--level long|hour|overnight|all] [--repeats N]
             [--timeout-secs N] [--json OUT]
```

Map levels exactly:

- `long` → `scenarios/long_drive_10min.toml`, default timeout 120 seconds;
- `hour` → `scenarios/soak_1hour.toml`, default timeout 300 seconds;
- `overnight` → `scenarios/overnight_8hour.toml`, default timeout 900 seconds;
- `all` → all three in that order.

Default level is `long`; default repeats is 3. Invoke the Rust `microcar` binary
with `--trace-retention 4096 --soak-summary-json -`. Add a specialized runner
that parses the single summary object and retains only the existing 20-line
stdout/stderr tails. Do not use `read_all_lines` or populate
`ScenarioRun.trace` for soak mode.

On Linux, sample `/proc/<pid>/status` `VmRSS` every 100 ms and record peak RSS.
The lane fails if:

- any run times out, panics, exits nonzero, or reports assertion failure;
- time regressions or unexpected frame loss/duplication are nonzero;
- repeat hashes or final states differ;
- retained records exceed 4,096;
- peak RSS exceeds 256 MiB;
- the 8-hour peak exceeds the 1-hour peak by more than 32 MiB.

On non-Linux hosts, omit only the RSS assertions and report them as unsupported;
all simulator, hash, state, and retention assertions remain required.

### J3. Unit, integration, and CI changes

Add tests for ring eviction, pre-eviction hashing, per-stream monotonicity,
requested-drop accounting, summary JSON parsing, timeout/panic handling, and a
fake-child test proving the soak runner retains bounded output.

`tests/run_all.sh` already skips the 1-hour and 8-hour scenarios unconditionally.
After the Rust soak lane is green, also remove the `RUN_LONG` Python execution
path, always skip all three long scenarios there, and change the skip message to
the exact replacement command: `harness soak --level <long|hour|overnight>`.

CI placement is fixed:

- PR-fast: no soak scenario;
- main: `harness soak --level long --repeats 1`;
- nightly: `harness soak --level all --repeats 3` with JSON artifact retained.

Stage J is complete when all three scenarios run through the Rust simulator,
the nightly command meets the bounds above, and no default or opt-in shell path
uses the Python simulator for a long/soak scenario.

## 15. CI and Verification

During development run the focused crate/lane first. Before declaring a stage
complete, run:

```bash
cd /home/zmm/projects/costar
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd /home/zmm/projects/microcar
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --bin microcar
./tests/run_all.sh
bash tests/verify_determinism.sh
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- toml-zoo
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- topology
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- diagnostics
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- charging
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- ota
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- cockpit
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- telematics
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- debug-gym
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- debug-gym-corpus
MICROCAR_BIN=target/debug/microcar cargo run -p microcar-dogfood --bin harness -- soak --level all --repeats 3
```

PR CI runs all unit tests, non-soak scenarios, topology, malformed input,
diagnostics, charging, OTA, cockpit, and `simfarm N=2`. Nightly additionally
runs 100-repeat isolation/determinism cases, `simfarm N=8` and `N=16`, the
64-node topology, long drive/soak, host TCP fragmentation/burst coverage,
complete debug corpus, the Stage J Rust soak lane/RSS bounds, and sanitizers
where supported.

## 16. Final Acceptance Checklist

Implementation is finished only when:

- two real Worlds and two concurrent real-firmware gRPC sessions match their
  solo traces and isolate device/network ID 0;
- restart preserves flash/EEPROM/block data, resets volatile state and C
  instance state, resumes firmware, and enforces the downtime delivery boundary;
- diagnostics values travel plant → BMS → gateway → tool over CAN with freshness;
- charging travels EVSE/BMS/gateway/powertrain/plant over CAN and changes real
  SOC/current/temperature while enforcing zero drive torque;
- OTA travels tool → gateway → flash → reboot/recovery and passes every matrix
  case using persistent metadata;
- cockpit streams nonempty deterministic frames and isolated touch/device state;
- telematics conserves framed records under every fragment boundary and burst;
- the 10-minute, 1-hour, and 8-hour scenarios pass through the bounded-memory
  Rust soak lane with repeatable hashes and final state;
- all seven debug seeds and all five typed predicate families pass;
- existing golden traces and legacy regression fixtures remain green;
- `UNBLOCKING.md`, `docs/BLOCKERS.md`, protocol/scenario documentation, and this
  guide are updated to describe the implemented behavior rather than historical
  intentions.

## 17. Prohibited Shortcuts

- Do not force vehicle mode or private gateway state from TOML.
- Do not count a trace marker as CAN delivery, display state, persistent reboot,
  BMS fault propagation, or OTA recovery.
- Do not replace per-machine ownership with a global map keyed by session ID.
- Do not use C thread-local storage for ECU instances.
- Do not let the charging or OTA harness directly invoke gateway helpers.
- Do not use host wall-clock ordering for deterministic virtual-network tests.
- Do not run long or soak scenarios through the Python trace generator.
- Do not remove a green legacy lane until its real replacement passes 100
  deterministic repetitions.
