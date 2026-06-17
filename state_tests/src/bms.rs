//! bms.rs — BMS state machine and torque limit tests.
//!
//! Validates:
//!   - Temperature threshold logic (OK / WARN_HOT / LIMP_REQUEST / CRITICAL_FAULT)
//!   - Torque limit computation based on BMS state

use super::protocol::*;

// ── BMS State Machine ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BmsStateMachine {
    pub state: BmsState,
    pub temp_c_x10: i16,
    pub voltage_mv: u16,
    pub current_ma: i16,
    pub soc_percent: u8,
    pub fault_code: BmsFaultCode,
    pub charger_plugged: bool,
}

impl Default for BmsStateMachine {
    fn default() -> Self {
        Self {
            state: BmsState::Ok,
            temp_c_x10: 250, // 25.0°C
            voltage_mv: 48000,
            current_ma: 0,
            soc_percent: 80,
            fault_code: BmsFaultCode::None,
            charger_plugged: false,
        }
    }
}

impl BmsStateMachine {
    pub fn update(
        &mut self,
        temp_c_x10: i16,
        voltage_mv: u16,
        current_ma: i16,
        soc_percent: u8,
    ) -> BmsState {
        self.temp_c_x10 = temp_c_x10;
        self.voltage_mv = voltage_mv;
        self.current_ma = current_ma;
        self.soc_percent = soc_percent;

        self.state = bms_determine_state(temp_c_x10);
        self.fault_code = self.determine_fault_code();
        self.state
    }

    fn determine_fault_code(&self) -> BmsFaultCode {
        match self.state {
            BmsState::CriticalFault => BmsFaultCode::Overtemp,
            _ => BmsFaultCode::None,
        }
    }

    pub fn is_fault(&self) -> bool {
        self.state == BmsState::CriticalFault
    }

    pub fn is_limp(&self) -> bool {
        self.state == BmsState::LimpRequest
    }
}

// ── BMS Limits ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BmsLimits {
    pub max_torque_percent: u8, // 255 = unlimited
    pub reason: u8,
}

impl Default for BmsLimits {
    fn default() -> Self {
        Self {
            max_torque_percent: 255,
            reason: 0,
        }
    }
}

impl BmsLimits {
    pub fn compute(&mut self, bms_state: BmsState) -> u8 {
        match bms_state {
            BmsState::Ok => {
                self.max_torque_percent = 255;
                self.reason = 0;
            }
            BmsState::WarnHot => {
                self.max_torque_percent = 255; // no limit yet, just warn
                self.reason = 1;
            }
            BmsState::LimpRequest => {
                self.max_torque_percent = MC_TORQUE_LIMP_MAX_PERCENT; // 25%
                self.reason = 2;
            }
            BmsState::CriticalFault => {
                self.max_torque_percent = MC_TORQUE_FAULT_MAX_PERCENT; // 0%
                self.reason = 3;
            }
        }
        self.max_torque_percent
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── BMS State Machine Tests ─────────────────────────────────────────

    #[test]
    fn test_bms_initial_state_ok() {
        let bms = BmsStateMachine::default();
        assert_eq!(bms.state, BmsState::Ok);
        assert_eq!(bms.fault_code, BmsFaultCode::None);
        assert!(!bms.is_fault());
        assert!(!bms.is_limp());
    }

    #[test]
    fn test_bms_temp_ok_range() {
        let mut bms = BmsStateMachine::default();

        // 25°C = 250 (0.1°C units)
        let state = bms.update(250, 48000, 0, 80);
        assert_eq!(state, BmsState::Ok);
        assert!(!bms.is_fault());

        // 60°C = 600 — still OK (boundary: ≤60 = OK)
        let state = bms.update(600, 48000, 0, 80);
        assert_eq!(state, BmsState::Ok);

        // 59°C = 590 — OK
        let state = bms.update(590, 48000, 0, 80);
        assert_eq!(state, BmsState::Ok);
    }

    #[test]
    fn test_bms_temp_warn_hot_range() {
        let mut bms = BmsStateMachine::default();

        // 61°C = 610 — WARN_HOT (boundary: >60)
        let state = bms.update(610, 48000, 0, 80);
        assert_eq!(state, BmsState::WarnHot);
        assert!(!bms.is_fault());
        assert!(!bms.is_limp());

        // 70°C = 700
        let state = bms.update(700, 48000, 0, 80);
        assert_eq!(state, BmsState::WarnHot);

        // 75°C = 750 — still WARN_HOT (boundary: ≤75)
        let state = bms.update(750, 48000, 0, 80);
        assert_eq!(state, BmsState::WarnHot);
    }

    #[test]
    fn test_bms_temp_limp_range() {
        let mut bms = BmsStateMachine::default();

        // 76°C = 760 — LIMP_REQUEST (boundary: >75)
        let state = bms.update(760, 48000, 0, 80);
        assert_eq!(state, BmsState::LimpRequest);
        assert!(!bms.is_fault());
        assert!(bms.is_limp());

        // 85°C = 850
        let state = bms.update(850, 48000, 0, 80);
        assert_eq!(state, BmsState::LimpRequest);

        // 90°C = 900 — still LIMP (boundary: ≤90)
        let state = bms.update(900, 48000, 0, 80);
        assert_eq!(state, BmsState::LimpRequest);
    }

    #[test]
    fn test_bms_temp_critical_fault_range() {
        let mut bms = BmsStateMachine::default();

        // 91°C = 910 — CRITICAL_FAULT (boundary: >90)
        let state = bms.update(910, 48000, 0, 80);
        assert_eq!(state, BmsState::CriticalFault);
        assert!(bms.is_fault());
        assert!(!bms.is_limp());

        // 120°C = 1200
        let state = bms.update(1200, 48000, 0, 80);
        assert_eq!(state, BmsState::CriticalFault);
    }

    #[test]
    fn test_bms_fault_code_critical() {
        let mut bms = BmsStateMachine::default();
        bms.update(910, 48000, 0, 80); // 91°C → CRITICAL_FAULT
        assert_eq!(bms.fault_code, BmsFaultCode::Overtemp);
    }

    #[test]
    fn test_bms_fault_code_none_for_non_critical() {
        let mut bms = BmsStateMachine::default();
        bms.update(750, 48000, 0, 80); // 75°C → WARN_HOT
        assert_eq!(bms.fault_code, BmsFaultCode::None);

        bms.update(850, 48000, 0, 80); // 85°C → LIMP_REQUEST
        assert_eq!(bms.fault_code, BmsFaultCode::None);
    }

    // ─── BMS Limits Tests ────────────────────────────────────────────────

    #[test]
    fn test_bms_limits_ok_unlimited() {
        let mut bl = BmsLimits::default();
        let limit = bl.compute(BmsState::Ok);
        assert_eq!(limit, 255); // unlimited
        assert_eq!(bl.reason, 0);
    }

    #[test]
    fn test_bms_limits_warn_hot_unlimited() {
        let mut bl = BmsLimits::default();
        let limit = bl.compute(BmsState::WarnHot);
        assert_eq!(limit, 255); // still unlimited
        assert_eq!(bl.reason, 1);
    }

    #[test]
    fn test_bms_limits_limp_request_25_percent() {
        let mut bl = BmsLimits::default();
        let limit = bl.compute(BmsState::LimpRequest);
        assert_eq!(limit, MC_TORQUE_LIMP_MAX_PERCENT); // 25%
        assert_eq!(bl.reason, 2);
    }

    #[test]
    fn test_bms_limits_critical_fault_zero() {
        let mut bl = BmsLimits::default();
        let limit = bl.compute(BmsState::CriticalFault);
        assert_eq!(limit, MC_TORQUE_FAULT_MAX_PERCENT); // 0%
        assert_eq!(bl.reason, 3);
    }

    #[test]
    fn test_bms_state_transitions_full_heat_cycle() {
        let mut bms = BmsStateMachine::default();

        // Start cold
        assert_eq!(bms.update(250, 48000, 0, 80), BmsState::Ok);

        // Heat up to WARN
        assert_eq!(bms.update(650, 48000, 0, 80), BmsState::WarnHot);

        // Heat up to LIMP
        assert_eq!(bms.update(800, 48000, 0, 80), BmsState::LimpRequest);

        // Heat up to CRITICAL
        assert_eq!(bms.update(950, 48000, 0, 80), BmsState::CriticalFault);

        // Cool down (still critical — state machine doesn't auto-recover)
        // Actually, the state is determined purely from temp, so it does recover
        assert_eq!(bms.update(800, 48000, 0, 80), BmsState::LimpRequest);
        assert_eq!(bms.update(650, 48000, 0, 80), BmsState::WarnHot);
        assert_eq!(bms.update(300, 48000, 0, 80), BmsState::Ok);
    }
}
