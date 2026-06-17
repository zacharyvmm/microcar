//! powertrain.rs — powertrain torque controller and safety rules tests.
//!
//! Validates:
//!   - brake overrides throttle (S1)
//!   - FAULT mode disables motor (S2)
//!   - LIMP mode caps torque at 25% (S3)
//!   - gateway watchdog timeout disables torque (S4)
//!   - invalid throttle detection (S6)

use super::protocol::*;

// ── Torque Controller ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TorqueController {
    pub throttle_percent: u8,
    pub brake_pressed: bool,
    pub gear: u8,
    pub vehicle_mode: VehicleMode,
    pub bms_torque_limit: u8, // 255 = unlimited
    pub output_torque: i8,
    pub motor_enable: bool,
}

impl Default for TorqueController {
    fn default() -> Self {
        Self {
            throttle_percent: 0,
            brake_pressed: false,
            gear: 0,
            vehicle_mode: VehicleMode::Off,
            bms_torque_limit: 255,
            output_torque: 0,
            motor_enable: true,
        }
    }
}

impl TorqueController {
    pub fn set_input(&mut self, throttle: u8, brake: bool, gear: u8) {
        self.throttle_percent = throttle;
        self.brake_pressed = brake;
        self.gear = gear;
    }

    pub fn set_mode(&mut self, mode: VehicleMode) {
        self.vehicle_mode = mode;
    }

    pub fn set_bms_limit(&mut self, max_torque: u8) {
        self.bms_torque_limit = max_torque;
    }

    pub fn compute(&mut self) -> i8 {
        // S6: invalid throttle → ignore
        if self.throttle_percent > 100 {
            self.output_torque = 0;
            self.motor_enable = false;
            return 0;
        }

        let requested = self.throttle_percent as i8;
        self.output_torque = safety_clamp_torque(
            requested,
            self.vehicle_mode,
            self.brake_pressed,
            self.bms_torque_limit,
        );

        // S2: FAULT mode → motor disabled
        self.motor_enable = self.vehicle_mode != VehicleMode::Fault;

        self.output_torque
    }

    pub fn is_throttle_invalid(&self) -> bool {
        self.throttle_percent > 100
    }
}

// ── Watchdog Task ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WatchdogTask {
    pub last_gateway_beat_ms: u32,
    pub gateway_online: bool,
    pub torque_disabled: bool,
}

impl Default for WatchdogTask {
    fn default() -> Self {
        Self {
            last_gateway_beat_ms: 0,
            gateway_online: false,
            torque_disabled: true, // disabled until first heartbeat
        }
    }
}

impl WatchdogTask {
    pub fn gateway_beat(&mut self, now_ms: u32) {
        self.last_gateway_beat_ms = now_ms;
        self.gateway_online = true;
        self.torque_disabled = false;
    }

    /// Check for gateway timeout. Returns true if torque should be disabled.
    pub fn check(&mut self, now_ms: u32) -> bool {
        if !self.gateway_online {
            self.torque_disabled = true;
            return true;
        }

        let elapsed = now_ms - self.last_gateway_beat_ms;
        if elapsed >= MC_SAFETY_GATEWAY_TIMEOUT_MS {
            self.gateway_online = false;
            self.torque_disabled = true;
            return true;
        }

        self.torque_disabled = false;
        false
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Torque Controller Tests ─────────────────────────────────────────

    #[test]
    fn test_torque_controller_initial_state() {
        let tc = TorqueController::default();
        assert_eq!(tc.throttle_percent, 0);
        assert!(!tc.brake_pressed);
        assert_eq!(tc.vehicle_mode, VehicleMode::Off);
        assert!(tc.motor_enable);
    }

    #[test]
    fn test_torque_controller_normal_drive() {
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_input(50, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 50);
        assert!(tc.motor_enable);
    }

    #[test]
    fn test_torque_controller_brake_overrides_throttle() {
        // S1: brake → torque=0
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_input(80, true, 1); // 80% throttle, brake pressed

        let torque = tc.compute();
        assert_eq!(torque, 0);
        // Motor should still be enabled (not a fault)
        assert!(tc.motor_enable);
    }

    #[test]
    fn test_torque_controller_fault_mode_disables_motor() {
        // S2: FAULT → motor disabled, torque=0
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Fault);
        tc.set_input(50, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 0);
        assert!(!tc.motor_enable);
    }

    #[test]
    fn test_torque_controller_limp_mode_caps_torque() {
        // S3: LIMP → capped at 25%
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Limp);
        tc.set_input(80, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 25);
        assert!(tc.motor_enable);
    }

    #[test]
    fn test_torque_controller_limp_below_cap_passes_through() {
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Limp);
        tc.set_input(10, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 10);
        assert!(tc.motor_enable);
    }

    #[test]
    fn test_torque_controller_bms_limit_caps_in_drive() {
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_bms_limit(50); // BMS limits to 50%
        tc.set_input(80, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 50);
    }

    #[test]
    fn test_torque_controller_bms_limit_below_input_passes() {
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_bms_limit(80);
        tc.set_input(50, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 50);
    }

    #[test]
    fn test_torque_controller_bms_limit_and_limp() {
        // BMS limit (10%) is lower than LIMP cap (25%)
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Limp);
        tc.set_bms_limit(10);
        tc.set_input(80, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 10);
    }

    #[test]
    fn test_torque_controller_invalid_throttle() {
        // S6: throttle >100% → ignored
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_input(150, false, 1); // invalid: >100%

        let torque = tc.compute();
        assert_eq!(torque, 0);
        assert!(!tc.motor_enable);
        assert!(tc.is_throttle_invalid());
    }

    #[test]
    fn test_torque_controller_invalid_throttle_exactly_100() {
        // 100% is valid
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_input(100, false, 1);

        assert!(!tc.is_throttle_invalid());
        let torque = tc.compute();
        assert_eq!(torque, 100);
    }

    #[test]
    fn test_torque_controller_invalid_throttle_101() {
        let mut tc = TorqueController::default();
        tc.set_input(101, false, 1);
        assert!(tc.is_throttle_invalid());
    }

    #[test]
    fn test_torque_controller_zero_throttle() {
        let mut tc = TorqueController::default();
        tc.set_mode(VehicleMode::Drive);
        tc.set_input(0, false, 1);

        let torque = tc.compute();
        assert_eq!(torque, 0);
        assert!(tc.motor_enable);
    }

    // ─── Watchdog Tests ──────────────────────────────────────────────────

    #[test]
    fn test_watchdog_initial_state_disabled() {
        let wd = WatchdogTask::default();
        assert!(!wd.gateway_online);
        assert!(wd.torque_disabled);
    }

    #[test]
    fn test_watchdog_gateway_beat_enables_torque() {
        let mut wd = WatchdogTask::default();
        wd.gateway_beat(100);
        assert!(wd.gateway_online);
        assert!(!wd.torque_disabled);

        let should_disable = wd.check(200);
        assert!(!should_disable);
        assert!(!wd.torque_disabled);
    }

    #[test]
    fn test_watchdog_timeout_disables_torque() {
        let mut wd = WatchdogTask::default();
        wd.gateway_beat(100);

        // At t=350: 250ms elapsed → timeout
        let should_disable = wd.check(350);
        assert!(should_disable);
        assert!(!wd.gateway_online);
        assert!(wd.torque_disabled);
    }

    #[test]
    fn test_watchdog_timeout_exact_boundary() {
        let mut wd = WatchdogTask::default();
        wd.gateway_beat(100);

        // At t=349: 249ms elapsed, still within timeout
        let should_disable = wd.check(349);
        assert!(!should_disable);
        assert!(wd.gateway_online);

        // At t=350: exactly 250ms
        let should_disable = wd.check(350);
        assert!(should_disable);
    }

    #[test]
    fn test_watchdog_no_initial_beat_always_disabled() {
        let mut wd = WatchdogTask::default();
        // Never received a heartbeat
        let should_disable = wd.check(1000);
        assert!(should_disable);
        assert!(wd.torque_disabled);
    }

    #[test]
    fn test_watchdog_reset() {
        let mut wd = WatchdogTask::default();
        wd.gateway_beat(100);
        assert!(wd.gateway_online);

        wd.reset();
        assert!(!wd.gateway_online);
        assert!(wd.torque_disabled);
    }
}
