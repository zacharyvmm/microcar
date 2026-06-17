# HANDOFF.md — costar Microcar Dogfood Demo

## Project Summary

We are building a separate demo repository named `microcar` next to the existing `costar` simulator repository.

Expected local paths:

```text
/home/zmm/projects/costar
/home/zmm/projects/microcar
```

The `microcar` project is a dogfood/demo project for `costar`. It should model a small distributed electric go-kart / micro-EV with multiple simulated ECUs connected by a deterministic CAN-like bus. The purpose is not automotive realism. The purpose is to force `costar` to grow into a credible deterministic distributed embedded simulation environment.

The agent may modify `costar` when needed, but must preserve separation of concerns:

* `microcar` contains application-specific firmware, message definitions, scenarios, plant model, and safety rules.
* `costar` contains generic simulator infrastructure, reusable virtual devices, scenario runner features, deterministic bus abstractions, tracing, and assertion mechanisms.
* Automotive-specific protocol details must stay in `microcar`.
* Generic embedded simulation primitives may be added to `costar` as crates or extensions.

The target is an agent-friendly, reproducible demo that can eventually be run with commands like:

```bash
cd /home/zmm/projects/microcar
cargo test
./tests/run_all.sh
costar scenario run scenarios/bms_overtemp_limp_mode.toml
```

If exact `costar` CLI commands do not exist yet, implement the smallest useful runner or script needed and document what is missing.

---

## High-Level Goal

Build a deterministic multi-ECU microcar demo with the following shape:

```text
driver inputs
    ↓
gateway ECU
    ↓
powertrain ECU → motor command → simulated vehicle plant
    ↑                                ↓
BMS ECU ← battery/temp/current sensors ← plant state
    ↓
dashboard ECU
```

Each ECU should be an independent simulated node. ECUs must communicate only through simulated buses/devices, not direct function calls.

The demo should eventually prove that `costar` can handle:

* multi-machine simulation
* deterministic virtual time
* FreeRTOS C firmware
* mixed Rust/C firmware
* eventually one Zephyr node
* deterministic CAN-like bus communication
* virtual sensors and actuators
* plant/environment simulation
* fault injection
* heartbeat supervision
* trace assertions
* golden traces
* long virtual-time scenarios
* CI-friendly reproducibility

---

## Non-Goals

Do not attempt to implement a realistic automotive stack.

Avoid these in the first version:

* AUTOSAR
* real ISO-TP / UDS diagnostics
* real CAN bit-level timing
* real motor physics
* real battery chemistry
* bootloaders
* OTA updates
* secure boot
* real board HAL compatibility
* graphical dashboard UI
* large numbers of ECUs
* external simulator GUI

This is a toy-scale distributed embedded system designed to pressure-test the simulator.

---

## Repository Boundary Rules

### `microcar` Owns

The `microcar` repository should own:

```text
/home/zmm/projects/microcar/
  firmware/
  common/
  plant/
  scenarios/
  expected/
  tests/
  docs/
```

It should include:

* ECU application code
* microcar-specific message protocol
* microcar-specific safety rules
* demo scenario definitions
* expected traces
* plant model for car speed, battery temperature, current draw, and state of charge
* test scripts
* documentation

### `costar` Owns

The `costar` repository should own generic simulator capabilities such as:

* multi-machine world orchestration
* deterministic link/bus infrastructure
* virtual device models
* generic CAN-like bus device, if implemented
* scenario parsing and execution infrastructure
* trace format and trace streaming
* assertion engine, if implemented generically
* fault injection primitives
* FreeRTOS/Zephyr runtime integration
* JSON-RPC or CLI simulator interface

### Rule of Thumb

If the code mentions `BMS_OVERTEMP`, `torque_percent`, `dashboard_warning`, or `microcar`, it belongs in `microcar`.

If the code mentions `CanFrame`, `BroadcastBus`, `FaultInjection`, `TraceAssertion`, `VirtualAdc`, `Machine`, `World`, or `Scenario`, it probably belongs in `costar`.

---

## Recommended `microcar` Repository Layout

Create this structure:

```text
/home/zmm/projects/microcar/
  HANDOFF.md
  README.md
  Cargo.toml

  common/
    include/
      microcar_protocol.h
      microcar_can.h
      microcar_trace.h
      microcar_safety.h
    src/
      microcar_protocol.c
      microcar_trace.c

  firmware/
    gateway_ecu/
      README.md
      src/
        main.c
        gateway_state.c
        gateway_state.h
        heartbeat_monitor.c
        heartbeat_monitor.h
        fault_manager.c
        fault_manager.h

    powertrain_ecu/
      README.md
      src/
        main.c
        torque_controller.c
        torque_controller.h
        safety_rules.c
        safety_rules.h
        watchdog_task.c
        watchdog_task.h

    bms_ecu/
      README.md
      src/
        main.c
        bms_state.c
        bms_state.h
        bms_limits.c
        bms_limits.h

    dashboard_ecu/
      README.md
      src/
        main.c
        dashboard_state.c
        dashboard_state.h
        warning_display.c
        warning_display.h

  plant/
    Cargo.toml
    src/
      lib.rs
      vehicle.rs
      battery.rs
      sensors.rs

  boards/
    gateway.toml
    powertrain.toml
    bms.toml
    dashboard.toml

  scenarios/
    boot_and_heartbeat.toml
    normal_drive_cycle.toml
    brake_overrides_throttle.toml
    bms_overtemp_limp_mode.toml
    critical_bms_shutdown.toml
    lost_bms_heartbeat.toml
    dashboard_reboot.toml
    gateway_reboot.toml
    dropped_vehicle_mode_frame.toml
    delayed_bms_fault_message.toml
    long_drive_10min.toml

  expected/
    traces/
      boot_and_heartbeat.trace
      normal_drive_cycle.trace
      brake_overrides_throttle.trace
      bms_overtemp_limp_mode.trace
      lost_bms_heartbeat.trace
    states/
      normal_drive_cycle.final.json
      bms_overtemp_limp_mode.final.json

  tests/
    run_all.sh
    run_one.sh
    check_trace.py
    check_assertions.py

  tools/
    trace_to_timeline.py
    trace_to_mermaid.py
    generate_expected_trace.sh

  docs/
    architecture.md
    message_protocol.md
    scenario_format.md
    safety_rules.md
    costar_requirements.md
```

Adjust as needed, but keep the separation between firmware, plant model, scenarios, expected output, and docs.

---

## Recommended `costar` Changes

Only modify `costar` when the capability is generic and useful beyond the microcar demo.

Potential costar changes:

```text
/home/zmm/projects/costar/crates/
  sim-can/          # optional new generic CAN-like bus crate
  sim-assert/       # optional generic scenario assertion crate
```

Potential extensions to existing crates:

```text
crates/sim-world/
  add broadcast bus support
  add fault injection hooks
  add plant/environment callback integration
  extend scenario TOML parsing

crates/sim-devices/
  add virtual ADC
  add watchdog device
  add PWM abstraction if needed

crates/sim-runner/
  add scenario CLI command support if missing
  add JSON trace output if missing
  add assertion reporting if missing
```

Do not add microcar-specific state machines or protocol constants to `costar`.

---

## Core Demo Architecture

The first version should include four ECUs:

| ECU              | Implementation                     | Purpose                                                    |
| ---------------- | ---------------------------------- | ---------------------------------------------------------- |
| `gateway_ecu`    | FreeRTOS C or Rust                 | central supervisor, heartbeat monitor, fault manager       |
| `powertrain_ecu` | FreeRTOS C                         | throttle/brake processing, motor command, torque limiting  |
| `bms_ecu`        | FreeRTOS C                         | battery state, temperature/current limits, fault reporting |
| `dashboard_ecu`  | FreeRTOS C initially, Zephyr later | display state, speed, warnings, heartbeat                  |

The first version can keep all ECUs in C/FreeRTOS if that is easiest. Later, convert `dashboard_ecu` to Zephyr to dogfood mixed-RTOS support.

---

## ECU Responsibilities

### Gateway ECU

The gateway is the central supervisor.

Responsibilities:

* receive heartbeats from all ECUs
* mark nodes online/offline
* aggregate faults
* broadcast vehicle mode
* route or rebroadcast important status
* enforce global safety state
* notify dashboard of warnings

Suggested states:

```c
typedef enum {
    VEHICLE_OFF = 0,
    VEHICLE_READY = 1,
    VEHICLE_DRIVE = 2,
    VEHICLE_LIMP = 3,
    VEHICLE_FAULT = 4,
    VEHICLE_CHARGING = 5
} vehicle_mode_t;
```

Important behavior:

| Condition                            | Expected Gateway Action         |
| ------------------------------------ | ------------------------------- |
| all required ECUs online             | enter `VEHICLE_READY`           |
| driver throttle active and no faults | enter `VEHICLE_DRIVE`           |
| BMS overtemperature warning          | enter `VEHICLE_LIMP`            |
| BMS critical fault                   | enter `VEHICLE_FAULT`           |
| BMS heartbeat lost                   | enter `VEHICLE_FAULT`           |
| powertrain heartbeat lost            | enter `VEHICLE_FAULT`           |
| dashboard heartbeat lost             | warning only, not drive-inhibit |
| invalid driver input                 | publish warning/fault           |
| charger plugged while driving        | enter `VEHICLE_FAULT`           |

---

### Powertrain ECU

The powertrain ECU is the primary FreeRTOS stress node.

Responsibilities:

* receive driver input
* receive vehicle mode from gateway
* receive BMS current/torque limits
* compute motor torque command
* publish powertrain status
* publish heartbeat
* enforce local safety rules
* shut down torque on timeout/fault

Suggested states:

```c
typedef enum {
    PT_DISABLED = 0,
    PT_READY = 1,
    PT_DRIVE = 2,
    PT_LIMP = 3,
    PT_FAULT = 4
} powertrain_state_t;
```

Required safety behavior:

| Condition                   | Expected Behavior              |
| --------------------------- | ------------------------------ |
| brake and throttle together | brake wins, torque = 0         |
| vehicle mode is `LIMP`      | torque capped                  |
| vehicle mode is `FAULT`     | torque = 0 and motor disabled  |
| BMS torque limit received   | torque capped                  |
| gateway command timeout     | torque = 0                     |
| invalid throttle value      | torque = 0 and fault published |
| watchdog task starved       | fault published                |

Use FreeRTOS primitives where possible:

* tasks
* queues
* timers
* event groups
* task notifications
* mutexes where justified

Avoid making the firmware too trivial. The goal is to stress the runtime.

---

### BMS ECU

The BMS ECU models battery management.

Responsibilities:

* read virtual battery temperature/current/voltage/SOC
* publish battery status
* publish torque/current limit
* request limp mode on high temperature
* request fault mode on critical temperature
* publish heartbeat

Suggested states:

```c
typedef enum {
    BMS_OK = 0,
    BMS_WARN_HOT = 1,
    BMS_LIMP_REQUEST = 2,
    BMS_CRITICAL_FAULT = 3
} bms_state_t;
```

Suggested thresholds:

| Condition    | BMS State            |
| ------------ | -------------------- |
| temp <= 60°C | `BMS_OK`             |
| temp > 60°C  | `BMS_WARN_HOT`       |
| temp > 75°C  | `BMS_LIMP_REQUEST`   |
| temp > 90°C  | `BMS_CRITICAL_FAULT` |

Suggested outputs:

* pack voltage
* pack current
* pack temperature
* state of charge
* max torque percent
* BMS fault code

---

### Dashboard ECU

The dashboard makes the demo understandable.

Responsibilities:

* receive vehicle mode
* receive speed
* receive battery state
* receive warnings
* emit trace-friendly display updates
* publish heartbeat

Suggested state:

```c
typedef struct {
    uint32_t speed_kph;
    uint8_t battery_percent;
    uint8_t vehicle_mode;
    bool limp_warning;
    bool fault_warning;
    bool bms_hot_warning;
} dashboard_state_t;
```

Expected trace output examples:

```text
dashboard: speed=12
dashboard: battery=78
dashboard: mode=LIMP
dashboard: warning=BMS_OVERTEMP
```

Later, this should be the first Zephyr-based ECU.

---

## Message Protocol

Create a small fixed-size protocol in:

```text
common/include/microcar_protocol.h
common/src/microcar_protocol.c
```

Suggested message IDs:

```c
#define MC_MSG_HEARTBEAT             0x001
#define MC_MSG_VEHICLE_MODE          0x010
#define MC_MSG_DRIVER_INPUT          0x020

#define MC_MSG_POWERTRAIN_STATUS     0x100
#define MC_MSG_MOTOR_COMMAND         0x101
#define MC_MSG_WHEEL_SPEED           0x102

#define MC_MSG_BMS_STATUS            0x200
#define MC_MSG_BMS_LIMITS            0x201
#define MC_MSG_BMS_FAULT             0x202

#define MC_MSG_DASHBOARD_STATUS      0x300
#define MC_MSG_WARNING               0x400
```

Suggested node IDs:

```c
#define MC_NODE_GATEWAY              1
#define MC_NODE_POWERTRAIN           2
#define MC_NODE_BMS                  3
#define MC_NODE_DASHBOARD            4
#define MC_NODE_PLANT                100
#define MC_NODE_TEST_HARNESS         200
```

Suggested payload structs:

```c
typedef struct {
    uint8_t node_id;
    uint32_t uptime_ms;
} mc_heartbeat_msg_t;

typedef struct {
    uint8_t throttle_percent;
    uint8_t brake_pressed;
    uint8_t gear;
} mc_driver_input_msg_t;

typedef struct {
    uint8_t mode;
    uint8_t fault_code;
} mc_vehicle_mode_msg_t;

typedef struct {
    uint16_t pack_voltage_mv;
    int16_t pack_current_ma;
    int16_t pack_temp_c_x10;
    uint8_t soc_percent;
} mc_bms_status_msg_t;

typedef struct {
    uint8_t max_torque_percent;
    uint8_t reason;
} mc_bms_limits_msg_t;

typedef struct {
    int8_t torque_percent;
    uint8_t enable;
} mc_motor_command_msg_t;

typedef struct {
    uint16_t speed_kph_x10;
} mc_wheel_speed_msg_t;

typedef struct {
    uint8_t source_node;
    uint8_t warning_code;
} mc_warning_msg_t;
```

Keep structs packed and fixed-size. Add explicit encode/decode helpers instead of relying on host ABI layout if portability becomes an issue.

---

## CAN-Like Bus

Start with a fake deterministic CAN-like broadcast bus.

Initial behavior:

* all ECUs attach to `vcan0`
* a frame has:

  * sender node ID
  * message ID
  * payload bytes
  * payload length
* frames are delivered to every attached node except sender
* fixed latency, e.g. 500 µs or 1 ms
* deterministic ordering by virtual time, priority, and sequence number

Do not implement full CAN arbitration at first.

Later behavior:

* lower CAN ID wins priority
* transmission time based on bitrate and payload length
* packet drop
* packet delay
* corruption
* bus-off state
* per-node filters
* error counters

### Where to Put This

If costar lacks a generic CAN/broadcast bus, create a generic `sim-can` crate in `costar`:

```text
/home/zmm/projects/costar/crates/sim-can/
  Cargo.toml
  src/
    lib.rs
    frame.rs
    bus.rs
    fault.rs
    trace.rs
```

Suggested generic types:

```rust
pub struct CanFrame {
    pub id: u32,
    pub sender: u64,
    pub data: [u8; 8],
    pub len: u8,
}

pub struct CanBusConfig {
    pub name: String,
    pub bitrate: u32,
    pub fixed_latency_ticks: u64,
    pub broadcast: bool,
}

pub enum CanFault {
    DropFrame { from: Option<u64>, id: Option<u32> },
    DelayFrame { extra_ticks: u64, id: Option<u32> },
    CorruptByte { index: usize, mask: u8, id: Option<u32> },
    DisconnectNode { node: u64 },
}
```

If adding `sim-can` is too large for the first pass, implement a minimal generic broadcast link in `sim-world` and document the need for `sim-can`.

---

## Vehicle Plant Model

The plant model belongs in `microcar`, not `costar`.

Create:

```text
/home/zmm/projects/microcar/plant/
  Cargo.toml
  src/lib.rs
  src/vehicle.rs
  src/battery.rs
  src/sensors.rs
```

Suggested state:

```rust
pub struct VehiclePlant {
    pub speed_kph: f32,
    pub battery_soc_percent: f32,
    pub battery_temp_c: f32,
    pub motor_temp_c: f32,
    pub throttle_percent: u8,
    pub brake_pressed: bool,
    pub motor_torque_percent: i8,
    pub charger_plugged: bool,
}
```

Suggested update loop:

```text
every 10 ms:
  speed += torque_percent * acceleration_factor
  speed -= brake_force if brake_pressed
  speed -= drag
  battery_soc -= current_draw
  battery_temp += current_draw_heat
  battery_temp -= cooling
  motor_temp += torque_heat
  motor_temp -= cooling
```

Keep the model intentionally simple and deterministic.

The plant should:

* receive motor torque command from powertrain
* receive driver input from scenario
* update speed and battery state
* inject wheel speed and battery sensor readings back into the bus
* support fault overrides from scenarios

Example plant faults:

* force battery temperature
* force stuck throttle
* force brake pressed
* freeze wheel speed sensor
* add sensor noise with deterministic seed
* force charger plugged

---

## Virtual Devices Needed

Minimum viable devices:

| Device           | Owner                    | Purpose                       |
| ---------------- | ------------------------ | ----------------------------- |
| CAN-like bus     | costar                   | ECU communication             |
| timer            | costar                   | periodic tasks and heartbeats |
| UART/debug trace | costar                   | readable logs                 |
| ADC/sensor input | costar or microcar shim  | BMS/pedal/battery readings    |
| watchdog         | costar or firmware logic | timeout behavior              |

Useful later devices:

| Device                 | Purpose                          |
| ---------------------- | -------------------------------- |
| virtual EEPROM         | persistent fault counters/config |
| virtual flash          | calibration/config               |
| PWM                    | motor command/light outputs      |
| GPIO                   | lights/warnings                  |
| virtual CAN controller | closer to real embedded APIs     |

If a virtual device is generic, implement it in `costar`. If it is just a microcar-specific input/output adapter, keep it in `microcar`.

---

## Scenario Format

The project should be scenario-first.

A scenario should describe:

* machines
* firmware targets
* bus topology
* plant model
* driver inputs
* faults
* duration
* expected events
* safety assertions
* expected final state

Target example:

```toml
name = "bms_overtemp_limp_mode"
duration_ms = 5000

[[machine]]
id = 1
name = "gateway"
firmware = "firmware/gateway_ecu"
rtos = "freertos"

[[machine]]
id = 2
name = "powertrain"
firmware = "firmware/powertrain_ecu"
rtos = "freertos"

[[machine]]
id = 3
name = "bms"
firmware = "firmware/bms_ecu"
rtos = "freertos"

[[machine]]
id = 4
name = "dashboard"
firmware = "firmware/dashboard_ecu"
rtos = "freertos"

[[bus]]
name = "vcan0"
type = "can"
latency_us = 500

[[bus.node]]
bus = "vcan0"
machine = "gateway"

[[bus.node]]
bus = "vcan0"
machine = "powertrain"

[[bus.node]]
bus = "vcan0"
machine = "bms"

[[bus.node]]
bus = "vcan0"
machine = "dashboard"

[plant]
type = "microcar"
tick_ms = 10

[[input]]
at_ms = 100
type = "driver_input"
throttle_percent = 50
brake_pressed = false

[[fault]]
at_ms = 1200
target = "plant.battery"
type = "force_temperature"
value_c = 82

[[expect.event]]
before_ms = 1500
machine = "gateway"
event = "vehicle_mode"
value = "LIMP"

[[expect.event]]
before_ms = 1600
machine = "powertrain"
event = "torque_limited"
max_percent = 25

[[expect.event]]
before_ms = 1700
machine = "dashboard"
event = "warning"
value = "BMS_OVERTEMP"
```

If costar’s scenario format does not yet support this, implement the minimal subset necessary for the first vertical slice and document the remaining required fields in `docs/costar_requirements.md`.

---

## Safety Rules

Create a dedicated document:

```text
docs/safety_rules.md
```

Safety rules should become scenario assertions.

Initial safety contract:

```text
S1: If brake is pressed, motor torque must be 0.
S2: If vehicle mode is FAULT, motor must be disabled.
S3: If BMS requests LIMP, torque must be capped within 100 ms.
S4: If gateway heartbeat is lost, powertrain must disable torque within 250 ms.
S5: If BMS heartbeat is lost, gateway must enter FAULT within 300 ms.
S6: If throttle input is invalid, powertrain must ignore it and publish a fault.
S7: Dashboard failure must not disable powertrain by itself.
S8: Critical BMS fault must shut down drive regardless of throttle.
```

Assertions can initially be checked by scripts over traces.

Later, if useful, add a generic assertion engine to `costar`.

---

## Required Scenarios

Implement scenarios in this order.

### 1. `boot_and_heartbeat`

Purpose:

* all ECUs boot
* all publish heartbeat
* gateway marks required nodes online
* gateway enters `VEHICLE_READY`

Expected events:

```text
gateway: node_online=bms
gateway: node_online=powertrain
gateway: node_online=dashboard
gateway: vehicle_mode=READY
```

This is the first scenario.

---

### 2. `normal_drive_cycle`

Purpose:

* driver applies throttle
* powertrain commands torque
* plant speed increases
* BMS publishes normal battery status
* dashboard displays speed and battery

Expected behavior:

```text
driver_input: throttle=30
powertrain: torque=30
plant: speed_increasing
dashboard: speed>0
```

---

### 3. `brake_overrides_throttle`

Purpose:

* validate local powertrain safety rule

Input:

```text
throttle = 80%
brake = pressed
```

Expected behavior:

```text
powertrain: torque=0
gateway: no_fault
dashboard: brake_indicator=on
```

---

### 4. `bms_overtemp_limp_mode`

Purpose:

* flagship distributed safety scenario

Flow:

```text
1. all ECUs boot
2. driver applies 50% throttle
3. powertrain commands torque
4. plant speed rises
5. scenario forces battery temperature to 82°C
6. BMS sends overtemp warning and torque limit
7. gateway enters LIMP mode
8. powertrain caps torque
9. dashboard shows BMS warning
```

Pass criteria:

```text
- gateway enters LIMP within 300 ms of BMS overtemperature
- powertrain torque <= 25% after LIMP
- dashboard emits BMS_OVERTEMP warning
- trace is deterministic
```

Build this as the first impressive demo.

---

### 5. `critical_bms_shutdown`

Input:

```text
battery temperature forced to 95°C
```

Expected behavior:

```text
bms: fault=CRITICAL_BMS_FAULT
gateway: mode=FAULT
powertrain: torque=0
dashboard: warning=CRITICAL_BMS_FAULT
```

---

### 6. `lost_bms_heartbeat`

Fault:

```text
BMS stops sending heartbeat
```

Expected behavior:

```text
gateway: node_lost=bms
gateway: mode=FAULT
powertrain: torque=0
dashboard: warning=BMS_OFFLINE
```

---

### 7. `dashboard_reboot`

Fault:

```text
dashboard reboots while car is driving
```

Expected behavior:

```text
gateway: dashboard_lost
gateway: vehicle_mode remains DRIVE
dashboard: resyncs state after reboot
```

Dashboard failure should not be drive-inhibiting by itself.

---

### 8. `gateway_reboot`

Fault:

```text
gateway reboots while driving
```

Expected behavior:

```text
powertrain: gateway_timeout
powertrain: torque=0
gateway: recovers
dashboard: warning=GATEWAY_RESTARTED
```

---

### 9. `dropped_vehicle_mode_frame`

Fault:

```text
drop one gateway → powertrain vehicle mode frame
```

Expected behavior:

```text
powertrain: no unsafe torque spike
powertrain: keeps previous safe mode or times out safely
```

---

### 10. `delayed_bms_fault_message`

Fault:

```text
delay BMS fault frame by 200 ms
```

Expected behavior:

```text
gateway eventually enters LIMP or FAULT
trace shows delayed propagation
system remains safe according to defined safety rules
```

---

### 11. `long_drive_10min`

Purpose:

* stress virtual-time fast-forward
* check deterministic long-run behavior

Expected behavior:

```text
simulate 10 minutes of virtual driving
final speed/SOC/temp deterministic
no wall-clock sleeps
same result across repeated runs
```

---

## Trace Format

Prefer structured traces, but human-readable traces are acceptable at first.

Human trace examples:

```text
t=000000 machine=gateway event=boot
t=000010 machine=bms event=heartbeat node=bms
t=000020 machine=powertrain event=heartbeat node=powertrain
t=000030 machine=gateway event=node_online node=bms
t=000100 machine=test event=driver_input throttle=50 brake=false
t=000120 machine=powertrain event=motor_command torque=50 enable=true
t=001200 machine=plant event=battery_temp temp_c=82
t=001250 machine=bms event=fault code=BMS_OVERTEMP
t=001300 machine=gateway event=vehicle_mode mode=LIMP
t=001350 machine=powertrain event=torque_limited max=25
t=001400 machine=dashboard event=warning code=BMS_OVERTEMP
```

JSONL trace examples:

```json
{"t":0,"machine":"gateway","event":"boot"}
{"t":100,"machine":"test","event":"driver_input","throttle":50,"brake":false}
{"t":1300,"machine":"gateway","event":"vehicle_mode","mode":"LIMP"}
```

If costar already has trace events, extend or adapt them. Do not discard existing tracing. Add microcar-level events through existing user trace hooks if possible.

---

## Assertions

Initially, assertions can be implemented in `microcar/tests/check_assertions.py`.

Later, if generic enough, move assertion evaluation into a `costar` crate such as `sim-assert`.

Suggested assertion syntax:

```toml
[[assert]]
name = "brake disables torque"
always_when = "brake_pressed == true"
condition = "motor_torque_percent == 0"

[[assert]]
name = "limp mode caps torque"
always_when = "vehicle_mode == LIMP"
condition = "motor_torque_percent <= 25"

[[assert]]
name = "bms overtemp reaches dashboard"
after_event = "bms.fault.BMS_OVERTEMP"
within_ms = 300
event = "dashboard.warning.BMS_OVERTEMP"
```

Do not overbuild the expression language at first. A Python checker over JSONL traces is acceptable for MVP.

---

## Build and Test Expectations

The agent should try to make both repos pass their tests after changes.

Suggested commands:

```bash
cd /home/zmm/projects/costar
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

```bash
cd /home/zmm/projects/microcar
cargo fmt --check || true
cargo test || true
./tests/run_all.sh
```

If some commands do not exist yet, create scripts or document missing work.

The first MVP should have at least one command that runs a scenario and prints PASS/FAIL.

Example target:

```bash
cd /home/zmm/projects/microcar
./tests/run_one.sh scenarios/bms_overtemp_limp_mode.toml
```

Expected output:

```text
PASS bms_overtemp_limp_mode
```

---

## MVP Vertical Slice

The first complete vertical slice should be:

```text
Scenario: bms_overtemp_limp_mode

Nodes:
  gateway_ecu
  powertrain_ecu
  bms_ecu
  dashboard_ecu

Flow:
  1. all ECUs boot
  2. driver applies 50% throttle
  3. powertrain commands torque
  4. plant speed rises
  5. scenario forces battery temperature to 82°C
  6. BMS sends overtemp warning and torque limit
  7. gateway enters LIMP mode
  8. powertrain caps torque
  9. dashboard shows warning

Pass:
  - limp mode reached within 300 ms
  - torque <= 25% after limp
  - dashboard warning emitted
  - trace deterministic across repeated runs
```

This scenario is the highest-value first milestone because it touches:

* multi-machine scheduling
* bus messages
* plant input
* sensor output
* BMS logic
* gateway supervision
* powertrain safety
* dashboard reporting
* trace assertions

---

## Phased Implementation Plan

### Phase 0 — Repository Setup

In `/home/zmm/projects/microcar`:

* create repository structure
* add `README.md`
* add this `HANDOFF.md`
* add basic docs
* add placeholder scenario files
* add `tests/run_all.sh`
* add `docs/costar_requirements.md`

Success criteria:

```text
microcar repo exists
README explains purpose
scenario list exists
costar integration requirements are documented
```

---

### Phase 1 — Protocol and Pure Logic

Implement microcar protocol and pure state-machine logic without deep costar integration.

Add:

* `microcar_protocol.h`
* message IDs
* payload structs
* encode/decode helpers
* gateway state machine
* powertrain torque rules
* BMS threshold logic
* dashboard state update logic

Success criteria:

```text
unit tests or simple host tests validate:
- brake overrides throttle
- BMS overtemp creates limp request
- gateway enters LIMP from BMS fault
- dashboard records warning
```

This phase ensures the demo logic is coherent before simulator integration.

---

### Phase 2 — Minimal costar Runner Integration

Run one ECU or simplified all-ECU logic under costar.

Acceptable first approach:

* run powertrain ECU only
* inject driver input
* observe motor command trace

Better approach:

* run all four ECUs as simulated machines
* use a minimal fake bus

Success criteria:

```text
a scenario can run and produce a deterministic trace
```

---

### Phase 3 — Multi-ECU Bus

Implement or reuse deterministic broadcast bus support.

If generic bus support is missing in costar, add one of:

1. minimal broadcast link extension in `sim-world`
2. new `sim-can` crate
3. temporary microcar-local bus with a clear TODO to move generic parts to costar

Preferred long-term direction: `costar/crates/sim-can`.

Success criteria:

```text
gateway, powertrain, bms, and dashboard exchange frames only through the bus
boot_and_heartbeat passes
normal_drive_cycle passes
```

---

### Phase 4 — Plant Model

Add deterministic plant model.

The plant should:

* receive driver input
* receive motor command
* update speed/SOC/temp
* publish wheel speed and battery sensor readings
* support scenario-controlled faults

Success criteria:

```text
normal_drive_cycle shows speed increasing
BMS receives battery temperature/current/SOC from plant
dashboard receives speed
```

---

### Phase 5 — Fault Injection

Add deterministic fault injection.

Initial faults:

* force battery temperature
* stop BMS heartbeat
* drop one message
* delay one message
* reboot dashboard
* reboot gateway

Success criteria:

```text
bms_overtemp_limp_mode passes
lost_bms_heartbeat passes
dashboard_reboot passes
gateway_reboot passes
```

If costar lacks fault injection support, implement minimal functionality in `microcar` first, then migrate generic pieces to costar.

---

### Phase 6 — Assertions and Golden Traces

Add:

* expected trace files
* final state snapshots
* assertion checker
* repeat-run determinism test

Success criteria:

```text
./tests/run_all.sh runs all implemented scenarios
same scenario run twice produces identical trace
semantic assertions pass
```

---

### Phase 7 — Zephyr Dashboard

Once FreeRTOS/C microcar works, convert or duplicate `dashboard_ecu` as a Zephyr-based node.

Do not make Zephyr responsible for critical safety behavior initially.

Success criteria:

```text
dashboard_ecu can run as Zephyr or Zephyr-like node
mixed RTOS scenario passes
```

---

## `costar` Work Items Likely Needed

Track these in `/home/zmm/projects/microcar/docs/costar_requirements.md`.

### Likely Needed Immediately

```text
[ ] Scenario support for machines with named firmware targets
[ ] Deterministic broadcast bus or CAN-like bus
[ ] Per-machine trace prefixes
[ ] Scenario input injection at virtual times
[ ] JSONL or machine-readable trace output
[ ] Easy way to run a scenario from external repo
```

### Likely Needed Soon

```text
[ ] Fault injection: drop/delay/corrupt bus frames
[ ] Node stop/reboot support
[ ] Plant/environment callback integration
[ ] Generic virtual ADC or sensor input
[ ] Scenario assertion support
[ ] Deterministic random seed support
```

### Likely Needed Later

```text
[ ] Virtual CAN controller device
[ ] CAN arbitration and bus load timing
[ ] Virtual EEPROM/flash
[ ] Watchdog device
[ ] Better Zephyr external app integration
[ ] Process isolation for unsafe C firmware
[ ] Trace viewer or timeline export
```

---

## Separation-of-Concerns Examples

### Belongs in `microcar`

```c
#define MC_MSG_BMS_FAULT 0x202
#define MC_FAULT_BMS_OVERTEMP 3
#define MC_NODE_POWERTRAIN 2
```

```rust
pub struct VehiclePlant {
    pub speed_kph: f32,
    pub battery_temp_c: f32,
}
```

```toml
name = "bms_overtemp_limp_mode"
```

### Belongs in `costar`

```rust
pub struct CanFrame {
    pub id: u32,
    pub sender: u64,
    pub data: [u8; 8],
    pub len: u8,
}
```

```rust
pub enum FrameFault {
    Drop,
    Delay { ticks: u64 },
    Corrupt { byte: usize, mask: u8 },
}
```

```rust
pub struct ScenarioAssertion {
    pub name: String,
    pub condition: AssertionCondition,
}
```

### Borderline

A simple plant callback system could be generic and belong in costar, but the actual car physics belong in microcar.

Generic:

```rust
trait EnvironmentModel {
    fn step(&mut self, now: Tick, world: &mut World);
}
```

Microcar-specific:

```rust
impl EnvironmentModel for VehiclePlant {
    fn step(...) {
        // update speed, battery temperature, SOC
    }
}
```

---

## Documentation Requirements

Create or maintain:

```text
README.md
docs/architecture.md
docs/message_protocol.md
docs/scenario_format.md
docs/safety_rules.md
docs/costar_requirements.md
```

The README should include:

* what the project is
* how it relates to costar
* how to run scenarios
* which scenarios currently pass
* which simulator features are still missing

Suggested README intro:

```markdown
# costar Microcar Demo

A deterministic distributed embedded simulation of a tiny electric go-kart.

This demo contains several simulated ECUs connected by a deterministic CAN-like bus:

- Gateway ECU
- Powertrain ECU
- Battery Management ECU
- Dashboard ECU

It dogfoods costar features including:

- multi-machine simulation
- FreeRTOS tasks, queues, timers, and event groups
- deterministic virtual time
- virtual sensors and actuators
- CAN-like message passing
- fault injection
- golden traces
- safety assertions
```

---

## Quality Bar

The project is successful when it can answer yes to most of these:

```text
[ ] Can four simulated ECUs run in one deterministic virtual world?
[ ] Do ECUs communicate only through simulated buses/devices?
[ ] Can a scenario inject driver inputs over time?
[ ] Can a plant model react to firmware outputs?
[ ] Can firmware read changing virtual sensors?
[ ] Can the simulator drop/delay/corrupt a bus message deterministically?
[ ] Can one ECU be rebooted or killed mid-scenario?
[ ] Can safety invariants be asserted over virtual time?
[ ] Can golden traces be compared in CI?
[ ] Can the same scenario run twice with identical output?
[ ] Can a long virtual-time scenario run quickly?
[ ] Is at least one node running FreeRTOS C?
[ ] Can one later node run Zephyr?
[ ] Do failures produce useful debug traces?
```

---

## Agent Instructions

When working on this project:

1. Inspect both repositories first:

   * `/home/zmm/projects/costar`
   * `/home/zmm/projects/microcar`

2. Do not merge microcar into costar.

3. Prefer path dependencies or CLI integration over copying costar code into microcar.

4. If costar lacks a generic capability, add it to costar only when it is reusable beyond microcar.

5. Keep microcar-specific protocol, plant model, and safety rules in microcar.

6. Start with the smallest vertical slice that produces a deterministic scenario trace.

7. Update `docs/costar_requirements.md` whenever a simulator feature is missing.

8. Run formatting and tests in both repos after changes.

9. Document any incomplete integration honestly.

10. Do not overbuild. The target is a useful dogfood demo, not a production automotive simulator.

---

## First Concrete Task List

Start with this task list.

### Task 1 — Create microcar skeleton

```text
[ ] create /home/zmm/projects/microcar
[ ] create README.md
[ ] create HANDOFF.md
[ ] create docs/
[ ] create common/include/microcar_protocol.h
[ ] create scenarios/
[ ] create expected/traces/
[ ] create tests/run_all.sh
```

### Task 2 — Define protocol

```text
[ ] define node IDs
[ ] define message IDs
[ ] define payload structs
[ ] add encode/decode helpers
[ ] add trace event naming conventions
```

### Task 3 — Implement pure state machines

```text
[ ] gateway heartbeat/fault state machine
[ ] powertrain torque/safety state machine
[ ] BMS threshold state machine
[ ] dashboard warning state machine
```

### Task 4 — Add first scenario file

```text
[ ] scenarios/bms_overtemp_limp_mode.toml
[ ] expected/traces/bms_overtemp_limp_mode.trace
[ ] expected/states/bms_overtemp_limp_mode.final.json
```

### Task 5 — Connect to costar

```text
[ ] determine existing costar scenario CLI/API
[ ] choose integration path
[ ] if missing, add minimal external scenario runner support
[ ] run one deterministic microcar scenario
```

### Task 6 — Add bus support

```text
[ ] determine whether sim-world links are enough
[ ] if not, add minimal broadcast bus support
[ ] consider costar/crates/sim-can if generic enough
[ ] route ECU messages through bus only
```

### Task 7 — Add assertions

```text
[ ] implement check_assertions.py or generic costar assertion support
[ ] assert LIMP reached within 300 ms
[ ] assert torque capped after LIMP
[ ] assert dashboard warning emitted
```

### Task 8 — Document gaps

```text
[ ] update docs/costar_requirements.md
[ ] list missing costar features discovered during implementation
[ ] separate must-have from nice-to-have
```

---

## Expected First Milestone

The first milestone is complete when this is possible:

```bash
cd /home/zmm/projects/microcar
./tests/run_one.sh scenarios/bms_overtemp_limp_mode.toml
```

Expected result:

```text
PASS bms_overtemp_limp_mode
```

And the run proves:

```text
- all four ECUs boot
- driver throttle is injected
- plant battery temperature is forced to 82°C
- BMS reports overtemperature
- gateway enters LIMP
- powertrain caps torque
- dashboard emits warning
- trace is deterministic across repeated runs
```

This is the highest-value first demo. Build this before expanding the project.

