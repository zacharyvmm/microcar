//! protocol.rs — message encode/decode and type definitions.
//!
//! Mirrors the C definitions in common/include/microcar_protocol.h,
//! common/include/microcar_safety.h, and microcar_trace.h.

#[allow(unused_imports)]
use std::mem;

// ── Node IDs ─────────────────────────────────────────────────────────────

pub const MC_NODE_GATEWAY: u8 = 1;
pub const MC_NODE_POWERTRAIN: u8 = 2;
pub const MC_NODE_BMS: u8 = 3;
pub const MC_NODE_DASHBOARD: u8 = 4;
pub const MC_NODE_PLANT: u8 = 100;
pub const MC_NODE_TEST_HARNESS: u8 = 200;

// ── Message IDs ──────────────────────────────────────────────────────────

pub const MC_MSG_HEARTBEAT: u32 = 0x001;
pub const MC_MSG_VEHICLE_MODE: u32 = 0x010;
pub const MC_MSG_DRIVER_INPUT: u32 = 0x020;
pub const MC_MSG_POWERTRAIN_STATUS: u32 = 0x100;
pub const MC_MSG_MOTOR_COMMAND: u32 = 0x101;
pub const MC_MSG_WHEEL_SPEED: u32 = 0x102;
pub const MC_MSG_BMS_STATUS: u32 = 0x200;
pub const MC_MSG_BMS_LIMITS: u32 = 0x201;
pub const MC_MSG_BMS_FAULT: u32 = 0x202;
pub const MC_MSG_DASHBOARD_STATUS: u32 = 0x300;
pub const MC_MSG_WARNING: u32 = 0x400;

// ── Vehicle Modes ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VehicleMode {
    Off = 0,
    Ready = 1,
    Drive = 2,
    Limp = 3,
    Fault = 4,
    Charging = 5,
}

impl VehicleMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => VehicleMode::Off,
            1 => VehicleMode::Ready,
            2 => VehicleMode::Drive,
            3 => VehicleMode::Limp,
            4 => VehicleMode::Fault,
            5 => VehicleMode::Charging,
            _ => VehicleMode::Off,
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            VehicleMode::Off => "OFF",
            VehicleMode::Ready => "READY",
            VehicleMode::Drive => "DRIVE",
            VehicleMode::Limp => "LIMP",
            VehicleMode::Fault => "FAULT",
            VehicleMode::Charging => "CHARGING",
        }
    }
}

// ── BMS States ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BmsState {
    Ok = 0,
    WarnHot = 1,
    LimpRequest = 2,
    CriticalFault = 3,
}

// ── BMS Fault Codes ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BmsFaultCode {
    None = 0,
    Overtemp = 1,
    Overvoltage = 2,
    Undervoltage = 3,
    OverCurrent = 4,
    CommError = 5,
}

// ── Warning Codes ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WarningCode {
    None = 0,
    BmsOvertemp = 1,
    BmsOffline = 2,
    PowertrainOffline = 3,
    GatewayRestarted = 4,
    DashboardOffline = 5,
    InvalidThrottle = 6,
    CriticalBmsFault = 7,
    ChargerPlugged = 8,
}

// ── Safety Constants ─────────────────────────────────────────────────────

pub const MC_SAFETY_LIMP_RESPONSE_MS: u32 = 300;
pub const MC_SAFETY_GATEWAY_TIMEOUT_MS: u32 = 250;
pub const MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS: u32 = 300;
pub const MC_SAFETY_HEARTBEAT_INTERVAL_MS: u32 = 100;
pub const MC_BMS_TEMP_OK: i16 = 60;
pub const MC_BMS_TEMP_WARN: i16 = 60;
pub const MC_BMS_TEMP_LIMP: i16 = 75;
pub const MC_BMS_TEMP_CRITICAL: i16 = 90;
pub const MC_TORQUE_LIMP_MAX_PERCENT: u8 = 25;
pub const MC_TORQUE_FAULT_MAX_PERCENT: u8 = 0;

// ── Payload Structures ───────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct HeartbeatMsg {
    pub node_id: u8,
    pub uptime_ms: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DriverInputMsg {
    pub throttle_percent: u8,
    pub brake_pressed: u8,
    pub gear: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VehicleModeMsg {
    pub mode: u8,
    pub fault_code: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct BmsStatusMsg {
    pub pack_voltage_mv: u16,
    pub pack_current_ma: i16,
    pub pack_temp_c_x10: i16,
    pub soc_percent: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct BmsLimitsMsg {
    pub max_torque_percent: u8,
    pub reason: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MotorCommandMsg {
    pub torque_percent: i8,
    pub enable: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WheelSpeedMsg {
    pub speed_kph_x10: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WarningMsg {
    pub source_node: u8,
    pub warning_code: u8,
}

// ── Encode/Decode Functions ──────────────────────────────────────────────

pub fn encode_heartbeat(node_id: u8, uptime_ms: u32) -> HeartbeatMsg {
    HeartbeatMsg { node_id, uptime_ms }
}

pub fn decode_heartbeat(msg: &HeartbeatMsg) -> (u8, u32) {
    (msg.node_id, msg.uptime_ms)
}

pub fn encode_vehicle_mode(mode: u8, fault_code: u8) -> VehicleModeMsg {
    VehicleModeMsg { mode, fault_code }
}

pub fn decode_vehicle_mode(msg: &VehicleModeMsg) -> (u8, u8) {
    (msg.mode, msg.fault_code)
}

pub fn encode_driver_input(throttle: u8, brake: u8, gear: u8) -> DriverInputMsg {
    DriverInputMsg {
        throttle_percent: throttle,
        brake_pressed: brake,
        gear,
    }
}

pub fn decode_driver_input(msg: &DriverInputMsg) -> (u8, u8, u8) {
    (msg.throttle_percent, msg.brake_pressed, msg.gear)
}

pub fn encode_bms_status(
    voltage_mv: u16,
    current_ma: i16,
    temp_c_x10: i16,
    soc_percent: u8,
) -> BmsStatusMsg {
    BmsStatusMsg {
        pack_voltage_mv: voltage_mv,
        pack_current_ma: current_ma,
        pack_temp_c_x10: temp_c_x10,
        soc_percent,
    }
}

pub fn decode_bms_status(msg: &BmsStatusMsg) -> (u16, i16, i16, u8) {
    (
        msg.pack_voltage_mv,
        msg.pack_current_ma,
        msg.pack_temp_c_x10,
        msg.soc_percent,
    )
}

pub fn encode_bms_limits(max_torque: u8, reason: u8) -> BmsLimitsMsg {
    BmsLimitsMsg {
        max_torque_percent: max_torque,
        reason,
    }
}

pub fn decode_bms_limits(msg: &BmsLimitsMsg) -> (u8, u8) {
    (msg.max_torque_percent, msg.reason)
}

pub fn encode_motor_command(torque: i8, enable: u8) -> MotorCommandMsg {
    MotorCommandMsg {
        torque_percent: torque,
        enable,
    }
}

pub fn decode_motor_command(msg: &MotorCommandMsg) -> (i8, u8) {
    (msg.torque_percent, msg.enable)
}

pub fn encode_wheel_speed(speed_kph_x10: u16) -> WheelSpeedMsg {
    WheelSpeedMsg { speed_kph_x10 }
}

pub fn decode_wheel_speed(msg: &WheelSpeedMsg) -> u16 {
    msg.speed_kph_x10
}

pub fn encode_warning(source: u8, code: u8) -> WarningMsg {
    WarningMsg {
        source_node: source,
        warning_code: code,
    }
}

pub fn decode_warning(msg: &WarningMsg) -> (u8, u8) {
    (msg.source_node, msg.warning_code)
}

// ── BMS State Determination ──────────────────────────────────────────────
//
// Thresholds in °C:
//   temp ≤ 60°C              → BMS_OK
//   60°C < temp ≤ 75°C       → BMS_WARN_HOT
//   75°C < temp ≤ 90°C       → BMS_LIMP_REQUEST
//   temp > 90°C              → BMS_CRITICAL_FAULT

pub fn bms_determine_state(temp_c_x10: i16) -> BmsState {
    // temp_c_x10 is in 0.1°C units
    let temp_c = temp_c_x10 / 10;

    if temp_c > MC_BMS_TEMP_CRITICAL {
        BmsState::CriticalFault
    } else if temp_c > MC_BMS_TEMP_LIMP {
        BmsState::LimpRequest
    } else if temp_c > MC_BMS_TEMP_WARN {
        BmsState::WarnHot
    } else {
        BmsState::Ok
    }
}

// ── Safety Torque Clamp ─────────────────────────────────────────────────

pub fn safety_clamp_torque(
    requested: i8,
    mode: VehicleMode,
    brake_pressed: bool,
    bms_max_torque: u8,
) -> i8 {
    // S1: brake overrides throttle
    if brake_pressed {
        return 0;
    }

    // S2: FAULT mode → motor disabled
    if mode == VehicleMode::Fault {
        return 0;
    }

    // S3: LIMP mode → capped at 25%
    let effective_max: u8 = match mode {
        VehicleMode::Limp => MC_TORQUE_LIMP_MAX_PERCENT.min(bms_max_torque),
        _ => bms_max_torque.min(100),
    };

    let clamped = requested.max(0).min(effective_max as i8);
    clamped
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_heartbeat() {
        let msg = encode_heartbeat(MC_NODE_GATEWAY, 12345);
        let (nid, uptime) = decode_heartbeat(&msg);
        assert_eq!(nid, MC_NODE_GATEWAY);
        assert_eq!(uptime, 12345);
        assert_eq!(mem::size_of::<HeartbeatMsg>(), 5); // packed: u8 + u32 = 5
    }

    #[test]
    fn test_encode_decode_vehicle_mode() {
        let msg = encode_vehicle_mode(VehicleMode::Drive as u8, 0);
        let (mode, fault) = decode_vehicle_mode(&msg);
        assert_eq!(mode, VehicleMode::Drive as u8);
        assert_eq!(fault, 0);
    }

    #[test]
    fn test_encode_decode_driver_input() {
        let msg = encode_driver_input(50, 0, 1);
        let (thr, brake, gear) = decode_driver_input(&msg);
        assert_eq!(thr, 50);
        assert_eq!(brake, 0);
        assert_eq!(gear, 1);
    }

    #[test]
    fn test_encode_decode_bms_status() {
        let msg = encode_bms_status(48000, 10000, 600, 80);
        let (v, c, t, s) = decode_bms_status(&msg);
        assert_eq!(v, 48000);
        assert_eq!(c, 10000);
        assert_eq!(t, 600);
        assert_eq!(s, 80);
    }

    #[test]
    fn test_encode_decode_bms_limits() {
        let msg = encode_bms_limits(25, 2);
        let (mt, r) = decode_bms_limits(&msg);
        assert_eq!(mt, 25);
        assert_eq!(r, 2);
    }

    #[test]
    fn test_encode_decode_motor_command() {
        let msg = encode_motor_command(30, 1);
        let (torque, en) = decode_motor_command(&msg);
        assert_eq!(torque, 30);
        assert_eq!(en, 1);
    }

    #[test]
    fn test_encode_decode_wheel_speed() {
        let msg = encode_wheel_speed(120);
        let speed = decode_wheel_speed(&msg);
        assert_eq!(speed, 120);
    }

    #[test]
    fn test_encode_decode_warning() {
        let msg = encode_warning(MC_NODE_BMS, WarningCode::BmsOvertemp as u8);
        let (src, code) = decode_warning(&msg);
        assert_eq!(src, MC_NODE_BMS);
        assert_eq!(code, WarningCode::BmsOvertemp as u8);
    }

    #[test]
    fn test_bms_state_from_temp() {
        // temp ≤ 60°C → BMS_OK
        assert_eq!(bms_determine_state(250), BmsState::Ok); // 25.0°C
        assert_eq!(bms_determine_state(600), BmsState::Ok); // 60.0°C (boundary)

        // 60°C < temp ≤ 75°C → BMS_WARN_HOT
        assert_eq!(bms_determine_state(610), BmsState::WarnHot); // 61.0°C
        assert_eq!(bms_determine_state(700), BmsState::WarnHot); // 70.0°C
        assert_eq!(bms_determine_state(750), BmsState::WarnHot); // 75.0°C (boundary)

        // 75°C < temp ≤ 90°C → BMS_LIMP_REQUEST
        assert_eq!(bms_determine_state(760), BmsState::LimpRequest); // 76.0°C
        assert_eq!(bms_determine_state(850), BmsState::LimpRequest); // 85.0°C
        assert_eq!(bms_determine_state(900), BmsState::LimpRequest); // 90.0°C (boundary)

        // temp > 90°C → BMS_CRITICAL_FAULT
        assert_eq!(bms_determine_state(910), BmsState::CriticalFault); // 91.0°C
        assert_eq!(bms_determine_state(1200), BmsState::CriticalFault); // 120.0°C
    }

    #[test]
    fn test_safety_clamp_torque_normal() {
        // Normal drive: full torque passes through
        assert_eq!(safety_clamp_torque(50, VehicleMode::Drive, false, 100), 50);
        assert_eq!(
            safety_clamp_torque(100, VehicleMode::Drive, false, 100),
            100
        );
    }

    #[test]
    fn test_safety_clamp_torque_brake_overrides() {
        // S1: brake → torque=0 regardless of mode
        assert_eq!(safety_clamp_torque(80, VehicleMode::Drive, true, 100), 0);
        assert_eq!(safety_clamp_torque(50, VehicleMode::Ready, true, 100), 0);
        assert_eq!(safety_clamp_torque(30, VehicleMode::Limp, true, 25), 0);
    }

    #[test]
    fn test_safety_clamp_torque_fault_mode() {
        // S2: FAULT mode → torque=0
        assert_eq!(safety_clamp_torque(80, VehicleMode::Fault, false, 100), 0);
        assert_eq!(safety_clamp_torque(5, VehicleMode::Fault, false, 100), 0);
    }

    #[test]
    fn test_safety_clamp_torque_limp_mode() {
        // S3: LIMP → capped at 25%
        assert_eq!(safety_clamp_torque(80, VehicleMode::Limp, false, 100), 25);
        assert_eq!(safety_clamp_torque(50, VehicleMode::Limp, false, 100), 25);
        assert_eq!(safety_clamp_torque(10, VehicleMode::Limp, false, 100), 10); // below cap
        assert_eq!(safety_clamp_torque(0, VehicleMode::Limp, false, 100), 0);
    }

    #[test]
    fn test_safety_clamp_torque_bms_limit() {
        // BMS limit of 50% should cap even in normal mode
        assert_eq!(safety_clamp_torque(80, VehicleMode::Drive, false, 50), 50);
        assert_eq!(safety_clamp_torque(40, VehicleMode::Drive, false, 50), 40);

        // BMS limit lower than LIMP cap should win
        assert_eq!(safety_clamp_torque(80, VehicleMode::Limp, false, 10), 10);
    }

    #[test]
    fn test_safety_clamp_torque_negative_input() {
        // Negative requested torque should be clamped to 0
        assert_eq!(safety_clamp_torque(-10, VehicleMode::Drive, false, 100), 0);
        assert_eq!(safety_clamp_torque(-50, VehicleMode::Ready, false, 100), 0);
    }

    #[test]
    fn test_safety_clamp_torque_off_ready_charging() {
        // OFF/READY/CHARGING — full torque allowed (subject to BMS limit)
        assert_eq!(safety_clamp_torque(30, VehicleMode::Off, false, 100), 30);
        assert_eq!(safety_clamp_torque(30, VehicleMode::Ready, false, 100), 30);
        assert_eq!(
            safety_clamp_torque(30, VehicleMode::Charging, false, 100),
            30
        );
    }
}
