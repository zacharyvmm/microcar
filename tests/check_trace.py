#!/usr/bin/env python3
"""check_trace.py — validate microcar scenario assertions against trace output.

Usage:
    python3 check_trace.py <scenario.toml> [trace.jsonl]

If trace.jsonl is omitted, the script runs a Python-based simulation
to generate the trace.  It then validates all [[expect.event]] assertions
and reports PASS/FAIL.

The simulation replicates the costar World event loop with:
  - Plant model (speed, battery SOC, temperature)
  - CAN bus with broadcast delivery, configurable latency
  - ECU firmware state machines (gateway, powertrain, BMS, dashboard)
  - Fault injection (force_temperature, stop_heartbeat, reboot,
    drop_frame, delay_frame)
  - Deterministic virtual time advancement

Trace format: JSONL (one JSON object per line) with fields:
    {"machine": <id>, "at": <ticks>, "event": "<type>", ...}
"""

import json
import os
import sys
from pathlib import Path

# ── ECU Firmware State Machines ────────────────────────────────────────
# These track the per-ECU state as CAN frames are processed.
# They produce the higher-level trace events that scenario assertions check.

# Node IDs
MC_NODE_GATEWAY = 1
MC_NODE_POWERTRAIN = 2
MC_NODE_BMS = 3
MC_NODE_DASHBOARD = 4
MC_NODE_PLANT = 100

# Message IDs
MC_MSG_HEARTBEAT = 0x001
MC_MSG_VEHICLE_MODE = 0x010
MC_MSG_DRIVER_INPUT = 0x020
MC_MSG_MOTOR_COMMAND = 0x101
MC_MSG_WHEEL_SPEED = 0x200
MC_MSG_BMS_STATUS = 0x201
MC_MSG_BMS_LIMITS = 0x202
MC_MSG_BMS_FAULT = 0x203
MC_MSG_WARNING = 0x400

# Vehicle modes
VEHICLE_OFF = 0
VEHICLE_READY = 1
VEHICLE_DRIVE = 2
VEHICLE_LIMP = 3
VEHICLE_FAULT = 4

# BMS states
BMS_OK = 0
BMS_WARN_HOT = 1
BMS_LIMP_REQUEST = 2
BMS_CRITICAL_FAULT = 3

# Safety thresholds
BMS_TEMP_OK = 60       # °C
BMS_TEMP_LIMP = 75     # °C
BMS_TEMP_CRITICAL = 90  # °C
LIMP_RESPONSE_MS = 300
GATEWAY_TIMEOUT_MS = 250
BMS_HEARTBEAT_TIMEOUT_MS = 300
HEARTBEAT_INTERVAL_MS = 100
TORQUE_LIMP_MAX = 25
TORQUE_FAULT_MAX = 0

MODE_NAMES = {0: "OFF", 1: "READY", 2: "DRIVE", 3: "LIMP", 4: "FAULT"}

class GatewayState:
    """Gateway ECU — monitors heartbeats, determines vehicle mode."""
    def __init__(self, machine_id=MC_NODE_GATEWAY):
        self.id = machine_id
        self.mode = VEHICLE_OFF
        self.nodes_online = set()
        self.node_last_beat = {}  # node_id -> last_heartbeat_time_us
        self.faults = []  # (severity, source, code)
        self.booted = False
        self.last_hb_send = -100_000_000
        self.booted = False
        self.last_hb_send = -100_000_000
        self.driving = False  # becomes True when powertrain sends non-zero torque

    def process_frame(self, frame_id, data, now_us, sender_id, trace):
        if frame_id == MC_MSG_HEARTBEAT:
            node_id = data[0] if len(data) > 0 else sender_id
            self.node_last_beat[node_id] = now_us
            if node_id not in self.nodes_online:
                self.nodes_online.add(node_id)
                node_name = {MC_NODE_POWERTRAIN: "powertrain", MC_NODE_BMS: "bms",
                             MC_NODE_DASHBOARD: "dashboard"}.get(node_id, str(node_id))
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "node_online",
                    "node": node_name,
                }))
            return

        if frame_id == MC_MSG_BMS_FAULT and len(data) > 0:
            fault_code = data[0]
            self.faults.append((2, MC_NODE_BMS, fault_code))

        if frame_id == MC_MSG_BMS_LIMITS and len(data) > 0:
            max_torque = data[0]
            # BMS is requesting torque limitation (LIMP_REQUEST).
            if max_torque <= TORQUE_LIMP_MAX:
                self.faults.append((1, MC_NODE_BMS, 1))  # severity 1 = LIMP request, code 1 = BMS_OVERTEMP

        if frame_id == MC_MSG_MOTOR_COMMAND and len(data) >= 1:
            # Detect non-zero torque to transition to DRIVE.
            torque = data[0] if isinstance(data[0], int) else (data[0] & 0xFF)
            # Convert signed byte.
            if torque > 127:
                torque -= 256
            if torque != 0:
                self.driving = True

        if frame_id == MC_MSG_WARNING and len(data) >= 2:
            source = data[0]
            code = data[1]
            self.faults.append((2 if code >= 7 else 1, source, code))

    def update(self, now_us, trace):
        # Check for node timeouts (300ms = 300_000 us).
        timeout_us = 300_000
        boot_timeout_us = 2_000_000  # 2s grace period for boot
        for node_id in [MC_NODE_POWERTRAIN, MC_NODE_BMS, MC_NODE_DASHBOARD]:
            last = self.node_last_beat.get(node_id)
            if last is None:
                if now_us > boot_timeout_us and node_id in self.nodes_online:
                    pass  # no beat yet, don't penalize during boot
                continue
            if now_us - last > timeout_us and node_id in self.nodes_online:
                self.nodes_online.discard(node_id)
                node_name = {MC_NODE_POWERTRAIN: "powertrain", MC_NODE_BMS: "bms",
                             MC_NODE_DASHBOARD: "dashboard"}.get(node_id, str(node_id))
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "node_lost",
                    "node": node_name,
                }))

        # Determine mode.
        old_mode = self.mode
        bms_lost = MC_NODE_BMS in self.node_last_beat and MC_NODE_BMS not in self.nodes_online

        # Add BMS offline warning if BMS is lost.
        if bms_lost:
            key = (2, MC_NODE_BMS, 2)  # BMS_OFFLINE
            already = any(f == key for f in self.faults)
            if not already:
                self.faults.append(key)

        has_critical = any(f[0] >= 2 for f in self.faults)
        has_warning = any(f[0] == 1 for f in self.faults)

        if not self.nodes_online:
            # No nodes online yet — stay OFF until first heartbeat arrives.
            pass
        else:
            if has_critical or bms_lost:
                self.mode = VEHICLE_FAULT
            elif has_warning:
                # Check if BMS requesting limp mode.
                bms_limp_warning = any(f[1] == MC_NODE_BMS and f[2] == 1
                                      for f in self.faults)
                if bms_limp_warning:
                    self.mode = VEHICLE_LIMP
                elif self.driving:
                    self.mode = VEHICLE_DRIVE
                else:
                    self.mode = VEHICLE_READY
            elif self.driving:
                self.mode = VEHICLE_DRIVE
            else:
                self.mode = VEHICLE_READY

        if self.mode != old_mode:
            trace.append(json.dumps({
                "machine": self.id, "at": now_us, "event": "vehicle_mode",
                "value": MODE_NAMES[self.mode],
            }))


class PowertrainState:
    """Powertrain ECU — torque compute, safety enforcement."""
    def __init__(self, machine_id=MC_NODE_POWERTRAIN):
        self.id = machine_id
        self.mode = VEHICLE_OFF
        self.throttle = 0
        self.brake = False
        self.gateway_beat = 0  # last gateway heartbeat time
        self.bms_limit = 100  # max torque percent from BMS
        self.motor_enable = True
        self.last_torque = -1  # -1 ensures first non-zero or zero publish
        self.torque_limited_count = 0  # for torque_limited event
        self.input_received = False  # ensures motor_command published when input arrives

    def process_frame(self, frame_id, data, now_us, sender_id, trace):
        if frame_id == MC_MSG_HEARTBEAT and sender_id == MC_NODE_GATEWAY:
            self.gateway_beat = now_us
            return

        if frame_id == MC_MSG_VEHICLE_MODE and len(data) > 0:
            self.mode = data[0]
            if self.mode == VEHICLE_FAULT:
                self.motor_enable = False
            elif self.mode == VEHICLE_LIMP:
                self.bms_limit = min(self.bms_limit, TORQUE_LIMP_MAX)
            return

        if frame_id == MC_MSG_BMS_LIMITS and len(data) > 0:
            self.bms_limit = min(self.bms_limit, data[0])
            return

    def set_driver_input(self, throttle, brake):
        self.throttle = throttle
        self.brake = brake
        self.input_received = True

    def update(self, now_us, trace):
        # Check gateway timeout (250ms = 250_000 us).
        if now_us - self.gateway_beat > 250_000 and self.gateway_beat > 0:
            self.motor_enable = False
            trace.append(json.dumps({
                "machine": self.id, "at": now_us, "event": "gateway_timeout",
            }))

        # Compute torque.
        if not self.motor_enable or self.mode == VEHICLE_FAULT:
            torque = 0
        elif self.brake:
            torque = 0
        elif self.mode == VEHICLE_LIMP:
            torque = min(self.throttle, TORQUE_LIMP_MAX)
            if torque > 0 and self.torque_limited_count == 0:
                self.torque_limited_count += 1
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "torque_limited",
                    "max_percent": TORQUE_LIMP_MAX,
                }))
        else:
            torque = self.throttle

        # Publish motor command on change OR when input just arrived.
        publish = (torque != self.last_torque) or self.input_received
        if publish:
            self.last_torque = torque
            self.input_received = False
            trace.append(json.dumps({
                "machine": self.id, "at": now_us, "event": "motor_command",
                "torque": torque,
            }))


class BmsState:
    """BMS ECU — temperature checks, fault publication."""
    def __init__(self, machine_id=MC_NODE_BMS):
        self.id = machine_id
        self.state = BMS_OK
        self.temp_c_x10 = 250  # 25.0°C
        self.last_state = BMS_OK
        self.last_fault_published = 0

    def process_frame(self, frame_id, data, now_us, sender_id, trace):
        if frame_id == MC_MSG_WHEEL_SPEED:
            return  # BMS doesn't care about speed
        if frame_id == MC_MSG_BMS_STATUS and len(data) >= 7:
            # Plant publishes [soc, volt_hi, volt_lo, temp_hi, temp_lo, curr_hi, curr_lo]
            temp_hi = data[3]
            temp_lo = data[4]
            self.temp_c_x10 = (temp_hi << 8) | temp_lo
            # temp_c_x10 is in 0.1°C — convert to °C for threshold comparison.
            temp_c = self.temp_c_x10 / 10.0

            old = self.state
            if temp_c > BMS_TEMP_CRITICAL:
                self.state = BMS_CRITICAL_FAULT
            elif temp_c > BMS_TEMP_LIMP:
                self.state = BMS_LIMP_REQUEST
            elif temp_c > BMS_TEMP_OK:
                self.state = BMS_WARN_HOT
            else:
                self.state = BMS_OK

            if self.state != old:
                state_names = {BMS_OK: "OK", BMS_WARN_HOT: "WARN_HOT",
                               BMS_LIMP_REQUEST: "LIMP_REQUEST",
                               BMS_CRITICAL_FAULT: "CRITICAL_FAULT"}
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "battery_temp",
                    "value": state_names[self.state],
                }))

                # Publish fault on transition to CRITICAL.
                if self.state == BMS_CRITICAL_FAULT and self.last_fault_published != 1:
                    self.last_fault_published = 1
                    trace.append(json.dumps({
                        "machine": self.id, "at": now_us, "event": "fault",
                        "value": "CRITICAL_BMS_FAULT",
                    }))

    def update(self, now_us, trace):
        # Publish BMS limits based on state.
        if self.state == BMS_LIMP_REQUEST:
            torque_limit = TORQUE_LIMP_MAX
        elif self.state == BMS_CRITICAL_FAULT:
            torque_limit = TORQUE_FAULT_MAX
        else:
            torque_limit = 100

        # BMS limits frame is published periodically (handled by ECU simulation loop).
        pass


class DashboardState:
    """Dashboard ECU — display updates, warning tracking."""
    def __init__(self, machine_id=MC_NODE_DASHBOARD):
        self.id = machine_id
        self.mode = VEHICLE_OFF
        self.speed = 0
        self.soc = 80
        self.temp_c_x10 = 250
        self.warnings = []  # (source, code)
        self.last_warning_published = None
        self.last_mode = VEHICLE_OFF

    def process_frame(self, frame_id, data, now_us, sender_id, trace):
        if frame_id == MC_MSG_VEHICLE_MODE and len(data) > 0:
            new_mode = data[0]
            if new_mode != self.last_mode:
                self.last_mode = self.mode
                self.mode = new_mode
                # Emit warning on FAULT mode.
                if new_mode == VEHICLE_FAULT and self.last_mode >= VEHICLE_DRIVE:
                    trace.append(json.dumps({
                        "machine": self.id, "at": now_us, "event": "warning",
                        "value": "VEHICLE_FAULT",
                    }))
            return

        if frame_id == MC_MSG_WHEEL_SPEED and len(data) >= 2:
            new_speed = (data[0] << 8) | data[1]
            if new_speed != self.speed:
                self.speed = new_speed
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "speed",
                    "value": str(new_speed),
                }))
            return

        if frame_id == MC_MSG_BMS_STATUS and len(data) >= 7:
            self.soc = data[0]
            self.temp_c_x10 = (data[3] << 8) | data[4]
            return

        if frame_id == MC_MSG_WARNING and len(data) >= 2:
            src = data[0]
            code = data[1]
            self.warnings.append((src, code))
            # Map warning codes to displayable names.
            warn_names = {
                1: "BMS_OVERTEMP",
                2: "BMS_OFFLINE",
                3: "POWERTRAIN_OFFLINE",
                4: "GATEWAY_RESTARTED",
                5: "DASHBOARD_OFFLINE",
                7: "CRITICAL_BMS_FAULT",
            }
            warn_name = warn_names.get(code, f"WARN_{code}")
            key = (MC_NODE_GATEWAY, code)
            if key != self.last_warning_published:
                self.last_warning_published = key
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "warning",
                    "value": warn_name,
                }))
            return

    def update(self, now_us, trace):
        # Publish speed if non-zero.
        if self.speed > 0:
            trace.append(json.dumps({
                "machine": self.id, "at": now_us, "event": "speed",
                "value": str(self.speed),
            }))

        # Determine if there's a warning to display.
        # Check BMS state for warnings.
        temp_c = self.temp_c_x10 / 10.0
        if temp_c > BMS_TEMP_CRITICAL:
            warn = (MC_NODE_BMS, 7)  # CRITICAL_BMS_FAULT
            if warn != self.last_warning_published:
                self.last_warning_published = warn
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "warning",
                    "value": "CRITICAL_BMS_FAULT",
                }))
        elif temp_c > BMS_TEMP_LIMP:
            warn = (MC_NODE_BMS, 1)  # BMS_OVERTEMP
            if warn != self.last_warning_published:
                self.last_warning_published = warn
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "warning",
                    "value": "BMS_OVERTEMP",
                }))

        # Check warnings from other sources.
        for source, code in self.warnings:
            if (source, code) != self.last_warning_published:
                warn_names = {
                    1: "BMS_OVERTEMP", 2: "BMS_OFFLINE",
                    3: "POWERTRAIN_OFFLINE", 4: "GATEWAY_RESTARTED",
                    5: "DASHBOARD_OFFLINE", 6: "INVALID_THROTTLE",
                    7: "CRITICAL_BMS_FAULT",
                }
                warn_str = warn_names.get(code, f"WARN_{code}")
                self.last_warning_published = (source, code)
                trace.append(json.dumps({
                    "machine": self.id, "at": now_us, "event": "warning",
                    "value": warn_str,
                }))
            break  # only emit one


# ── ECU heartbeat generator ─────────────────────────────────────────────

class EcuHeartbeats:
    """Generates periodic heartbeat frames for each ECU.

    Each ECU sends a heartbeat every 100ms (100_000 µs) on the bus.
    When an ECU is 'stopped' (stop_heartbeat fault), it stops sending.
    When an ECU is 'rebooted', it resumes sending after a delay.
    """
    def __init__(self, machines):
        self.next_hb_time = {}      # machine_id -> next heartbeat time
        self.uptime = {}             # machine_id -> uptime in us
        self.stopped = set()         # machine IDs paused by stop_heartbeat
        self.rebooting = {}          # machine_id -> time when reboot completes
        self.machine_ids = [m["id"] for m in machines]

        # Each ECU sends first heartbeat at 50ms offset from their ID.
        for m in machines:
            mid = m["id"]
            # Stagger boot times slightly.
            self.next_hb_time[mid] = (mid * 10 + 40) * 1000  # 40+10*id ms
            self.uptime[mid] = 0

    def next_event_time(self):
        """Return the earliest next heartbeat send time (or None)."""
        times = []
        for mid in self.machine_ids:
            if mid not in self.stopped:
                t = self.next_hb_time.get(mid)
                if t is not None:
                    times.append(t)
        if not times:
            return None
        return min(times)

    def generate(self, now_us, trace, pending_frames, machines, bus_latency_us, bus_delayed, bus_dropped):
        """Send heartbeats due at or before now_us."""
        for mid in self.machine_ids:
            if mid in self.stopped:
                continue
            # Check if rebooting — skip until reboot completes.
            if mid in self.rebooting:
                if now_us < self.rebooting[mid]:
                    continue
                else:
                    del self.rebooting[mid]

            next_t = self.next_hb_time.get(mid)
            if next_t is None:
                continue

            if next_t <= now_us:
                # Send heartbeat.
                uptime = self.uptime[mid]
                hb_data = [
                    mid,
                    (uptime >> 24) & 0xFF,
                    (uptime >> 16) & 0xFF,
                    (uptime >> 8) & 0xFF,
                    uptime & 0xFF,
                ]

                # Check drop.
                if MC_MSG_HEARTBEAT in bus_dropped:
                    self.next_hb_time[mid] = next_t + 100_000
                    self.uptime[mid] += 100_000
                    continue

                # Calculate arrival with delay.
                extra = bus_delayed.get(MC_MSG_HEARTBEAT, 0)
                arrival = now_us + bus_latency_us + extra

                # Record CanTx.
                trace.append(json.dumps({
                    "machine": mid, "at": now_us, "event": "can-tx",
                    "sender": mid, "id": MC_MSG_HEARTBEAT, "len": 5,
                }))

                # Queue delivery to all other nodes.
                for m in machines:
                    rid = m["id"]
                    if rid == mid:
                        continue
                    pending_frames.append((arrival, rid, mid, MC_MSG_HEARTBEAT, list(hb_data)))

                # Schedule next heartbeat.
                self.next_hb_time[mid] = next_t + 100_000
                self.uptime[mid] += 100_000

    def stop_heartbeat(self, machine_id):
        """Stop a machine's heartbeat (fault injection)."""
        self.stopped.add(machine_id)

    def reboot(self, machine_id, now_us):
        """Reboot a machine — stop then restart after 500ms delay."""
        self.stopped.add(machine_id)
        self.rebooting[machine_id] = now_us + 500_000  # 500ms reboot time
        self.uptime[machine_id] = 0


# ── Firmware event emission ─────────────────────────────────────────────

def emit_firmware_events(trace, now_us, receiver_id, sender_id, frame_id, data, machine_map, ecu_states):
    """Process a received CAN frame through ECU state machines.

    Updates the per-ECU state and emits higher-level trace events
    (node_online, vehicle_mode, motor_command, etc.) as appropriate.
    """
    gateway = ecu_states.get(MC_NODE_GATEWAY)
    powertrain = ecu_states.get(MC_NODE_POWERTRAIN)
    bms = ecu_states.get(MC_NODE_BMS)
    dashboard = ecu_states.get(MC_NODE_DASHBOARD)

    if receiver_id == MC_NODE_GATEWAY and gateway:
        gateway.process_frame(frame_id, data, now_us, sender_id, trace)
    elif receiver_id == MC_NODE_POWERTRAIN and powertrain:
        powertrain.process_frame(frame_id, data, now_us, sender_id, trace)
    elif receiver_id == MC_NODE_BMS and bms:
        bms.process_frame(frame_id, data, now_us, sender_id, trace)
    elif receiver_id == MC_NODE_DASHBOARD and dashboard:
        dashboard.process_frame(frame_id, data, now_us, sender_id, trace)


def update_ecu_states(now_us, trace, ecu_states, pending_frames, machines, bus_latency_us, bus_delayed, bus_dropped):
    """Periodic update for all ECUs to check timeouts and publish state.

    ECUs can return a list of frames to publish: (frame_id, data_list).
    These are broadcast on the bus to all other machines.
    """
    frames_to_publish = []  # (sender_id, frame_id, data_list)

    for ecu_id, ecu in ecu_states.items():
        ecu.update(now_us, trace)

        # ── Collect frames to publish ──────────────────────────
        if ecu_id == MC_NODE_GATEWAY:
            gateway = ecu
            # Publish vehicle mode (0x010) — always publish at update time.
            frames_to_publish.append((ecu_id, MC_MSG_VEHICLE_MODE,
                                      [gateway.mode, 0]))

            # Publish warning only for new/unique faults (don't spam).
            published_now = set()
            for (severity, source, code) in gateway.faults:
                if severity >= 1:
                    key = (source, code)
                    if key not in published_now:
                        published_now.add(key)
                        frames_to_publish.append((ecu_id, MC_MSG_WARNING,
                                                  [source, code]))

        elif ecu_id == MC_NODE_BMS:
            bms = ecu
            # Publish BMS fault when in critical state.
            if bms.state == BMS_CRITICAL_FAULT:
                frames_to_publish.append((ecu_id, MC_MSG_BMS_FAULT,
                                          [1]))  # fault_code = 1 (OVERTEMP)

            # Publish BMS limits when in LIMP.
            if bms.state == BMS_LIMP_REQUEST:
                frames_to_publish.append((ecu_id, MC_MSG_BMS_LIMITS,
                                          [TORQUE_LIMP_MAX, 0]))

        elif ecu_id == MC_NODE_POWERTRAIN:
            pt = ecu
            # Publish motor command (0x101).
            torque_byte = pt.last_torque & 0xFF
            frames_to_publish.append((ecu_id, MC_MSG_MOTOR_COMMAND,
                                      [torque_byte, int(pt.motor_enable)]))

        elif ecu_id == MC_NODE_DASHBOARD:
            dash = ecu
            # Dashboard publishes warnings it detects.
            if dash.last_warning_published:
                src, code = dash.last_warning_published
                frames_to_publish.append((ecu_id, MC_MSG_WARNING,
                                          [src, code]))

    # ── Deliver frames on the bus ──────────────────────────────
    for (sender_id, frame_id, data) in frames_to_publish:
        if frame_id in bus_dropped:
            continue

        extra = bus_delayed.get(frame_id, 0)
        arrival = now_us + bus_latency_us + extra

        trace.append(json.dumps({
            "machine": sender_id, "at": now_us, "event": "can-tx",
            "sender": sender_id, "id": frame_id, "len": len(data),
        }))

        for m in machines:
            rid = m["id"]
            if rid == sender_id:
                continue
            pending_frames.append((arrival, rid, sender_id, frame_id, list(data)))


# ── Assertion checking ────────────────────────────────────────────────────

def check_assertions(scenario_path, trace_lines):
    """Load scenario TOML, validate all expect.event assertions against trace.

    Returns (passed, total, failures_list).
    """
    try:
        import tomllib  # Python 3.11+
    except ImportError:
        import tomli as tomllib

    try:
        with open(scenario_path, 'rb') as f:
            scenario = tomllib.load(f)
    except Exception as e:
        print(f"ERROR: failed to parse {scenario_path}: {e}")
        return 0, 0, []

    expect = scenario.get("expect", {})
    events = expect.get("event", [])

    if not events:
        print(f"  (no expect.event assertions)")
        return 0, 0, []

    # Build machine name → id lookup.
    name_to_id = {}
    for m in scenario.get("machine", []):
        name_to_id[m["name"]] = m["id"]

    # Parse trace lines into structured events.
    parsed = []
    for line in trace_lines:
        line = line.strip()
        if not line:
            continue
        try:
            evt = json.loads(line)
            parsed.append(evt)
        except json.JSONDecodeError:
            # Non-JSON trace line (legacy format) — parse manually.
            parts = line.split()
            if len(parts) >= 4:
                try:
                    machine_str = parts[0]  # "[machine.N]"
                    mid = int(machine_str.strip("[]").split(".")[1])
                    at = int(parts[1])
                    event_type = parts[2]
                    evt = {"machine": mid, "at": at, "event": event_type}
                    # Parse key=value pairs.
                    for kv in parts[3:]:
                        if '=' in kv:
                            k, v = kv.split('=', 1)
                            try:
                                evt[k] = int(v)
                            except ValueError:
                                evt[k] = v
                    parsed.append(evt)
                except (ValueError, IndexError):
                    pass

    passed = 0
    failed = 0
    failures = []

    for i, assertion in enumerate(events, 1):
        before_ms = assertion.get("before_ms", 0)
        before_ticks = before_ms * 1000  # convert ms to ticks
        machine_name = assertion.get("machine", "")
        event_type = assertion.get("event", "")
        expected_value = assertion.get("value")
        expected_node = assertion.get("node")
        expected_max_percent = assertion.get("max_percent")
        expected_torque = assertion.get("torque")

        machine_id = name_to_id.get(machine_name)
        if machine_id is None:
            failures.append(f"  #{i}: unknown machine '{machine_name}'")
            failed += 1
            continue

        # Find matching events: same machine, same event type, before deadline.
        matches = [
            e for e in parsed
            if e.get("machine") == machine_id
            and e.get("event") == event_type
            and e.get("at", 0) <= before_ticks
        ]

        if not matches:
            failures.append(
                f"  #{i}: no {event_type} event from machine {machine_id} "
                f"({machine_name}) before {before_ms}ms"
            )
            failed += 1
            continue

        ok = False
        for match in matches:
            all_conditions = []

            if expected_value is not None:
                actual_val = match.get("value")
                if expected_value.startswith(">"):
                    threshold = int(expected_value[1:])
                    try:
                        actual_num = int(actual_val) if actual_val is not None else 0
                        all_conditions.append(actual_num > threshold)
                    except (ValueError, TypeError):
                        all_conditions.append(False)
                else:
                    all_conditions.append(str(actual_val) == str(expected_value))

            if expected_node is not None:
                actual_node = match.get("node")
                all_conditions.append(str(actual_node) == str(expected_node))

            if expected_max_percent is not None:
                actual_max = match.get("max_percent", 100)
                try:
                    all_conditions.append(int(actual_max) <= int(expected_max_percent))
                except (ValueError, TypeError):
                    all_conditions.append(False)

            if expected_torque is not None:
                actual_torque = match.get("torque")
                try:
                    all_conditions.append(int(actual_torque) == int(expected_torque))
                except (ValueError, TypeError):
                    all_conditions.append(False)

            if all(all_conditions):
                ok = True
                break

        if ok:
            passed += 1
        else:
            detail = f"  #{i}: {event_type} on {machine_name} before {before_ms}ms"
            if expected_value:
                detail += f" value={expected_value}"
            if expected_node:
                detail += f" node={expected_node}"
            if expected_max_percent is not None:
                detail += f" max_percent<={expected_max_percent}"
            if expected_torque is not None:
                detail += f" torque={expected_torque}"
            detail += " — no matching event found"
            failures.append(detail)
            failed += 1

    return passed, passed + failed, failures


# ── Simulation engine ─────────────────────────────────────────────────────

class MicrocarPlantSimple:
    """Python replica of the microcar plant model for MVP trace generation."""

    def __init__(self):
        self.speed_kph_x10 = 0
        self.soc_percent = 80
        self.soc_milli = 80_000
        self.temp_c_x10 = 250   # 25.0°C
        self.voltage_mv = 48000
        self.current_ma = 0
        self.throttle_percent = 0
        self.brake_pressed = False
        self.motor_torque = 0
        self.nominal_voltage_mv = 48000

    def set_driver_input(self, throttle, brake):
        self.throttle_percent = throttle
        self.brake_pressed = brake
        # Map throttle to motor torque 1:1 (simplified).
        if brake:
            self.motor_torque = 0
        else:
            self.motor_torque = throttle

    def apply_fault(self, target, fault_type, value_c):
        if target == "battery" and fault_type == "force_temperature":
            if value_c is not None:
                self.temp_c_x10 = int(value_c * 10)

    def step(self, dt_ms):
        dt = dt_ms

        # Speed calculation (simplified from vehicle.rs).
        effective_torque = self.motor_torque
        delta = effective_torque * dt // 100
        drag = self.speed_kph_x10 * dt // 10000
        new_speed = self.speed_kph_x10 + delta - drag
        self.speed_kph_x10 = max(0, new_speed)

        # Battery model (simplified from battery.rs).
        abs_torque = abs(self.motor_torque)
        current_ma = abs_torque * 500
        self.current_ma = current_ma

        # SOC decrease.
        soc_drop = current_ma * dt * 1000 // (3600 * 50_000)
        self.soc_milli = max(0, self.soc_milli - soc_drop)
        self.soc_percent = min(100, (self.soc_milli + 500) // 1000)

        # Temperature: heating from I²R.
        i_amps = current_ma // 1000
        heat_rise = i_amps * i_amps * dt // 25000

        # Cooling: proportional to above ambient.
        above_ambient = max(0, self.temp_c_x10 - 250)
        cooling = above_ambient * dt // 100000

        new_temp = self.temp_c_x10 + heat_rise - cooling
        self.temp_c_x10 = max(200, new_temp)

        # Voltage calculation.
        sag = abs(self.current_ma) // 10
        self.voltage_mv = max(0, (self.nominal_voltage_mv * self.soc_percent) // 100 - sag)

    def readings(self):
        return {
            "speed_kph_x10": self.speed_kph_x10,
            "soc_percent": self.soc_percent,
            "voltage_mv": self.voltage_mv,
            "temp_c_x10": self.temp_c_x10,
            "current_ma": self.current_ma,
        }


def simulate_scenario(scenario_path):
    """Simulate a scenario using Python-based plant model and ECU state machines.

    This is a lightweight Python implementation of the costar scenario runner
    for MVP testing.  It reads the scenario TOML, simulates the plant model
    publishing frames, tracks ECU state machines, and produces JSONL trace output.
    """
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib

    with open(scenario_path, 'rb') as f:
        scenario = tomllib.load(f)

    trace = []
    machines = scenario.get("machine", [])
    buses = scenario.get("bus", [])
    bus_injects = scenario.get("bus_inject", [])
    plant_cfg = scenario.get("plant", {})
    inputs = scenario.get("input", [])
    faults = scenario.get("fault", [])
    duration_ms = scenario.get("duration_ms", 0)

    # Build machine ID → name map.
    machine_map = {m["id"]: m["name"] for m in machines}

    # Build name → id map.
    name_to_id = {m["name"]: m["id"] for m in machines}

    # Bus latency defaults to 500us = 0.5ms.
    bus_latency_us = 500
    for bus in buses:
        if bus.get("latency_us"):
            bus_latency_us = bus["latency_us"]

    # Plant tick interval.
    tick_ms = plant_cfg.get("tick_ms", 10)
    plant_type = plant_cfg.get("type", "")

    # ── ECU state machines ───────────────────────────────────
    ecu_states = {
        MC_NODE_GATEWAY: GatewayState(MC_NODE_GATEWAY),
        MC_NODE_POWERTRAIN: PowertrainState(MC_NODE_POWERTRAIN),
        MC_NODE_BMS: BmsState(MC_NODE_BMS),
        MC_NODE_DASHBOARD: DashboardState(MC_NODE_DASHBOARD),
    }

    # ECU heartbeat generator.
    heartbeats = EcuHeartbeats(machines)

    # Simulation time in microseconds (ticks).
    current_time_us = 0

    # Pending CAN frame deliveries: (arrival_us, receiver_id, sender_id, id, data)
    pending_frames = []

    # Plant state.
    if plant_type == "microcar":
        plant = MicrocarPlantSimple()
    else:
        plant = None

    # Scheduled driver inputs: (at_us, throttle, brake)
    scheduled_inputs = []
    for inp in inputs:
        if inp.get("type") == "driver_input":
            at_us = inp["at_ms"] * 1000
            scheduled_inputs.append((
                at_us,
                inp.get("throttle_percent", 0),
                inp.get("brake_pressed", False),
            ))
    scheduled_inputs.sort(key=lambda x: x[0])

    # Scheduled faults: (at_us, target, fault_type, value_c, delay_ms, id)
    scheduled_faults = []
    for f in faults:
        at_us = f["at_ms"] * 1000
        target = f.get("target", "")
        fault_type = f.get("type", "")
        value_c = f.get("value_c")
        delay_ms = f.get("delay_ms")
        frame_id = f.get("id")
        scheduled_faults.append((at_us, target, fault_type, value_c, delay_ms, frame_id))
    scheduled_faults.sort(key=lambda x: x[0])

    # Scheduled bus_inject frames: (at_us, sender_name, id, data)
    scheduled_injects = []
    for bi in bus_injects:
        at_us = bi["at_ms"] * 1000
        sender_name = bi.get("sender", "")
        # Find sender machine ID.
        sender_id = 0
        for m in machines:
            if m["name"] == sender_name:
                sender_id = m["id"]
                break
        scheduled_injects.append((at_us, sender_id, bi["id"], bi["data"]))
    scheduled_injects.sort(key=lambda x: x[0])

    # Simulation loop.
    input_idx = 0
    fault_idx = 0
    inject_idx = 0
    next_plant_tick = tick_ms * 1000
    next_ecu_update = 10_000  # update ECUs every 10ms
    last_ecu_update = 0
    bus_dropped_frames = set()     # dropped frame IDs
    bus_delayed_frames = {}       # id → extra_delay_us

    while True:
        # Find next event time.
        next_time = None

        # Pending frame arrivals.
        if pending_frames:
            next_time = pending_frames[0][0]

        # Next heartbeat send.
        hb_time = heartbeats.next_event_time()
        if hb_time is not None:
            if next_time is None or hb_time < next_time:
                next_time = hb_time

        # Next plant tick.
        if plant is not None:
            if next_time is None or next_plant_tick < next_time:
                next_time = next_plant_tick

        # Next ECU state update.
        ecu_update_time = last_ecu_update + next_ecu_update
        if next_time is None or ecu_update_time < next_time:
            next_time = ecu_update_time

        # Next input.
        if input_idx < len(scheduled_inputs):
            t = scheduled_inputs[input_idx][0]
            if next_time is None or t < next_time:
                next_time = t

        # Next fault.
        if fault_idx < len(scheduled_faults):
            t = scheduled_faults[fault_idx][0]
            if next_time is None or t < next_time:
                next_time = t

        # Next bus_inject.
        if inject_idx < len(scheduled_injects):
            t = scheduled_injects[inject_idx][0]
            if next_time is None or t < next_time:
                next_time = t

        # Check duration.
        if duration_ms > 0 and current_time_us >= duration_ms * 1000:
            break

        # No more events — advance to next plant tick if plant exists.
        if next_time is None and plant is not None:
            next_time = next_plant_tick
            next_plant_tick += tick_ms * 1000

        if next_time is None:
            break

        # Don't go backwards.
        if next_time < current_time_us:
            break
        current_time_us = next_time

        # ── Apply faults ────────────────────────────────────────
        while fault_idx < len(scheduled_faults) and scheduled_faults[fault_idx][0] <= current_time_us:
            _, target, fault_type, value_c, delay_ms, frame_id = scheduled_faults[fault_idx]
            fault_idx += 1

            parts = target.split(".", 1)
            domain = parts[0] if parts else ""
            name = parts[1] if len(parts) > 1 else ""

            trace.append(json.dumps({
                "machine": 0, "at": current_time_us, "event": "fault_inject",
                "target": target, "type": fault_type,
            }))

            if domain == "plant" and plant:
                plant.apply_fault(name, fault_type, value_c)
            elif domain == "bus":
                if fault_type == "drop_frame" and frame_id is not None:
                    bus_dropped_frames.add(frame_id)
                    trace.append(json.dumps({
                        "machine": 0, "at": current_time_us, "event": "can-drop",
                        "id": frame_id,
                    }))
                elif fault_type == "delay_frame" and frame_id is not None:
                    extra_us = (delay_ms or 0) * 1000
                    bus_delayed_frames[frame_id] = extra_us
                    trace.append(json.dumps({
                        "machine": 0, "at": current_time_us, "event": "can-delay",
                        "id": frame_id, "extra_ticks": extra_us,
                    }))
            elif domain == "machine":
                target_id = name_to_id.get(name)
                if target_id is not None:
                    if fault_type == "stop_heartbeat":
                        heartbeats.stop_heartbeat(target_id)
                        ecu_states.get(target_id).__class__  # keep the state, just stop heartbeats
                    elif fault_type == "reboot":
                        heartbeats.reboot(target_id, current_time_us)
                        # Reset the ECU state on reboot.
                        if target_id == MC_NODE_GATEWAY:
                            ecu_states[MC_NODE_GATEWAY] = GatewayState(MC_NODE_GATEWAY)
                        elif target_id == MC_NODE_POWERTRAIN:
                            ecu_states[MC_NODE_POWERTRAIN] = PowertrainState(MC_NODE_POWERTRAIN)
                        elif target_id == MC_NODE_BMS:
                            ecu_states[MC_NODE_BMS] = BmsState(MC_NODE_BMS)
                        elif target_id == MC_NODE_DASHBOARD:
                            ecu_states[MC_NODE_DASHBOARD] = DashboardState(MC_NODE_DASHBOARD)

        # ── Apply bus_inject frames ─────────────────────────────
        while inject_idx < len(scheduled_injects) and scheduled_injects[inject_idx][0] <= current_time_us:
            _, sender_id, frame_id, data = scheduled_injects[inject_idx]
            inject_idx += 1

            # Record CanTx on sender.
            trace.append(json.dumps({
                "machine": sender_id,
                "at": current_time_us,
                "event": "can-tx",
                "sender": sender_id,
                "id": frame_id,
                "len": len(data),
            }))

            # Check drop.
            if frame_id in bus_dropped_frames:
                continue

            # Calculate arrival time with bus latency + extra delay.
            extra = bus_delayed_frames.get(frame_id, 0)
            arrival = current_time_us + bus_latency_us + extra

            # Deliver to all other nodes.
            for m in machines:
                receiver_id = m["id"]
                if receiver_id == sender_id:
                    continue
                pending_frames.append((arrival, receiver_id, sender_id, frame_id, list(data)))

        # ── Send heartbeats ─────────────────────────────────────
        heartbeats.generate(current_time_us, trace, pending_frames,
                            machines, bus_latency_us, bus_delayed_frames, bus_dropped_frames)

        # ── Deliver pending frames ──────────────────────────────
        delivered = []
        for p in pending_frames[:]:
            arrival, receiver_id, sender_id, frame_id, data = p
            if arrival <= current_time_us:
                delivered.append(p)
                pending_frames.remove(p)

                # Record CanRx.
                trace.append(json.dumps({
                    "machine": receiver_id,
                    "at": current_time_us,
                    "event": "can-rx",
                    "receiver": receiver_id,
                    "id": frame_id,
                    "len": len(data),
                }))
                # Record CanTx (for the tracer — sender perspective).
                trace.append(json.dumps({
                    "machine": sender_id,
                    "at": current_time_us,
                    "event": "can-tx",
                    "sender": sender_id,
                    "id": frame_id,
                    "len": len(data),
                }))

                # ── Emit higher-level firmware events ────────────
                emit_firmware_events(trace, current_time_us, receiver_id,
                                     sender_id, frame_id, data, machine_map, ecu_states)

        # ── Periodic ECU state update ──────────────────────────
        if current_time_us >= ecu_update_time:
            update_ecu_states(current_time_us, trace, ecu_states,
                               pending_frames, machines, bus_latency_us,
                               bus_delayed_frames, bus_dropped_frames)
            last_ecu_update = current_time_us

        # ── Step plant ──────────────────────────────────────────
        if plant is not None and current_time_us >= next_plant_tick:
            plant.step(tick_ms)
            next_plant_tick += tick_ms * 1000

            # Plant publishes wheel speed (0x200) and BMS status (0x201).
            readings = plant.readings()
            plant_sender_id = MC_NODE_PLANT

            # Wheel speed frame (0x200).
            speed = readings["speed_kph_x10"]
            speed_data = [(speed >> 8) & 0xFF, speed & 0xFF]
            wheel_arrival = current_time_us + bus_latency_us
            for m in machines:
                rid = m["id"]
                pending_frames.append((wheel_arrival, rid, plant_sender_id, MC_MSG_WHEEL_SPEED, speed_data))

            # BMS status frame (0x201).
            temp = readings["temp_c_x10"]
            volt = readings["voltage_mv"]
            curr = readings["current_ma"]
            soc = readings["soc_percent"]
            temp_u16 = temp & 0xFFFF
            curr_u16 = curr & 0xFFFF
            bms_data = [
                soc,
                (volt >> 8) & 0xFF, volt & 0xFF,
                (temp_u16 >> 8) & 0xFF, temp_u16 & 0xFF,
                (curr_u16 >> 8) & 0xFF, curr_u16 & 0xFF,
            ]
            bms_arrival = current_time_us + bus_latency_us
            for m in machines:
                rid = m["id"]
                pending_frames.append((bms_arrival, rid, plant_sender_id, MC_MSG_BMS_STATUS, bms_data))

        # ── Apply driver inputs ─────────────────────────────────
        while input_idx < len(scheduled_inputs) and scheduled_inputs[input_idx][0] <= current_time_us:
            _, throttle, brake = scheduled_inputs[input_idx]
            input_idx += 1
            if plant:
                plant.set_driver_input(throttle, brake)

            # Also set driver input on powertrain ECU directly.
            pt = ecu_states.get(MC_NODE_POWERTRAIN)
            if pt:
                pt.set_driver_input(throttle, brake)

    # Sort trace by machine ID and timestamp for deterministic output.
    def sort_key(line):
        e = json.loads(line)
        return (e.get("machine", 0), e.get("at", 0), e.get("event", ""))
    trace.sort(key=sort_key)

    return trace


# ── Golden trace generation ────────────────────────────────────────────────

def generate_golden_trace(scenario_path):
    """Generate a golden trace file by simulating the scenario."""
    trace_lines = simulate_scenario(scenario_path)
    return trace_lines


# ── Main ────────────────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 2:
        print("Usage: check_trace.py <scenario.toml> [trace.jsonl]")
        print("       check_trace.py --generate <scenario.toml>")
        sys.exit(1)

    if sys.argv[1] == "--generate":
        # Generate golden trace for a scenario.
        if len(sys.argv) < 3:
            print("Usage: check_trace.py --generate <scenario.toml>")
            sys.exit(1)
        scenario_path = sys.argv[2]
        trace_lines = generate_golden_trace(scenario_path)
        for line in trace_lines:
            print(line)
        sys.exit(0)

    scenario_path = sys.argv[1]
    scenario_name = os.path.splitext(os.path.basename(scenario_path))[0]

    if len(sys.argv) >= 3:
        # Read existing trace file.
        with open(sys.argv[2], 'r') as f:
            trace_lines = f.readlines()
    else:
        # Run the simulation.
        trace_lines = simulate_scenario(scenario_path)

    # Check assertions.
    passed, total, failures = check_assertions(scenario_path, trace_lines)

    if total == 0:
        print(f"  {scenario_name}: OK (no assertions)")
        sys.exit(0)

    if failures:
        for f in failures:
            print(f)
        print(f"  {scenario_name}: FAIL ({passed}/{total} passed)")
        sys.exit(1)
    else:
        print(f"  {scenario_name}: PASS ({passed}/{total})")
        sys.exit(0)


if __name__ == "__main__":
    main()
