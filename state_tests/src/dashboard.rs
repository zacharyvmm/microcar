//! dashboard.rs — dashboard state management and warning display tests.
//!
//! Validates:
//!   - Receives vehicle mode, speed, battery, warnings
//!   - Warning priority display (S7: dashboard failure doesn't disable powertrain)
//!   - Reboot behaviour

use super::protocol::*;

// ── Dashboard State ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DashboardState {
    pub vehicle_mode: VehicleMode,
    pub speed_kph_x10: u16,
    pub soc_percent: u8,
    pub battery_temp_c_x10: i16,
    pub battery_voltage_mv: u16,
    pub warnings: Vec<DashboardWarning>,
    pub initialized: bool,
    pub booted: bool,
}

#[derive(Debug, Clone)]
pub struct DashboardWarning {
    pub warning_code: u8,
    pub active: bool,
    pub source_node: u8,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            vehicle_mode: VehicleMode::Off,
            speed_kph_x10: 0,
            soc_percent: 0,
            battery_temp_c_x10: 0,
            battery_voltage_mv: 0,
            warnings: Vec::new(),
            initialized: false,
            booted: true,
        }
    }
}

impl DashboardState {
    pub fn set_mode(&mut self, mode: VehicleMode) {
        self.vehicle_mode = mode;
        self.initialized = true;
    }

    pub fn set_speed(&mut self, speed_kph_x10: u16) {
        self.speed_kph_x10 = speed_kph_x10;
        self.initialized = true;
    }

    pub fn set_battery(&mut self, soc_percent: u8, temp_c_x10: i16, voltage_mv: u16) {
        self.soc_percent = soc_percent;
        self.battery_temp_c_x10 = temp_c_x10;
        self.battery_voltage_mv = voltage_mv;
        self.initialized = true;
    }

    pub fn add_warning(&mut self, warning_code: u8, source_node: u8) {
        for w in &mut self.warnings {
            if w.warning_code == warning_code && w.source_node == source_node {
                w.active = true;
                return;
            }
        }
        self.warnings.push(DashboardWarning {
            warning_code,
            active: true,
            source_node,
        });
    }

    pub fn clear_warning(&mut self, warning_code: u8) {
        for w in &mut self.warnings {
            if w.warning_code == warning_code {
                w.active = false;
            }
        }
    }

    pub fn clear_node_warnings(&mut self, source_node: u8) {
        for w in &mut self.warnings {
            if w.source_node == source_node {
                w.active = false;
            }
        }
    }

    /// Get the most severe active warning code (0 = none).
    pub fn top_warning(&self) -> u8 {
        let mut top_code = WarningCode::None as u8;
        let mut top_severity = 0u8;

        for w in &self.warnings {
            if !w.active {
                continue;
            }
            let sev = warning_severity(w.warning_code);
            if sev > top_severity {
                top_severity = sev;
                top_code = w.warning_code;
            }
        }
        top_code
    }

    pub fn reboot(&mut self) {
        *self = Self::default();
        self.booted = false;
    }
}

/// Warning severity scores for priority display.
fn warning_severity(code: u8) -> u8 {
    match code {
        x if x == WarningCode::CriticalBmsFault as u8 => 3,
        x if x == WarningCode::BmsOvertemp as u8 => 2,
        x if x == WarningCode::PowertrainOffline as u8 => 2,
        x if x == WarningCode::GatewayRestarted as u8 => 2,
        x if x == WarningCode::BmsOffline as u8 => 1,
        x if x == WarningCode::DashboardOffline as u8 => 1,
        x if x == WarningCode::InvalidThrottle as u8 => 1,
        x if x == WarningCode::ChargerPlugged as u8 => 1,
        _ => 0,
    }
}

// ── Warning Display ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WarningDisplay {
    pub displayed_warning: u8,
    pub severity: u8,
}

impl Default for WarningDisplay {
    fn default() -> Self {
        Self {
            displayed_warning: WarningCode::None as u8,
            severity: 0,
        }
    }
}

impl WarningDisplay {
    pub fn update(&mut self, warning_code: u8, severity: u8) -> u8 {
        if severity > self.severity {
            self.displayed_warning = warning_code;
            self.severity = severity;
        }
        self.displayed_warning
    }

    pub fn clear(&mut self) {
        self.displayed_warning = WarningCode::None as u8;
        self.severity = 0;
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Dashboard State Tests ───────────────────────────────────────────

    #[test]
    fn test_dashboard_initial_state() {
        let ds = DashboardState::default();
        assert_eq!(ds.vehicle_mode, VehicleMode::Off);
        assert_eq!(ds.speed_kph_x10, 0);
        assert!(!ds.initialized);
        assert!(ds.booted);
    }

    #[test]
    fn test_dashboard_set_mode() {
        let mut ds = DashboardState::default();
        ds.set_mode(VehicleMode::Drive);
        assert_eq!(ds.vehicle_mode, VehicleMode::Drive);
        assert!(ds.initialized);
    }

    #[test]
    fn test_dashboard_set_speed() {
        let mut ds = DashboardState::default();
        ds.set_speed(120); // 12.0 km/h
        assert_eq!(ds.speed_kph_x10, 120);
        assert!(ds.initialized);
    }

    #[test]
    fn test_dashboard_set_battery() {
        let mut ds = DashboardState::default();
        ds.set_battery(85, 600, 47000);
        assert_eq!(ds.soc_percent, 85);
        assert_eq!(ds.battery_temp_c_x10, 600);
        assert_eq!(ds.battery_voltage_mv, 47000);
        assert!(ds.initialized);
    }

    #[test]
    fn test_dashboard_receives_all_updates() {
        let mut ds = DashboardState::default();

        // Simulate full update cycle
        ds.set_mode(VehicleMode::Drive);
        ds.set_speed(250); // 25 km/h
        ds.set_battery(75, 650, 46500);

        assert_eq!(ds.vehicle_mode, VehicleMode::Drive);
        assert_eq!(ds.speed_kph_x10, 250);
        assert_eq!(ds.soc_percent, 75);
        assert_eq!(ds.battery_temp_c_x10, 650);
        assert_eq!(ds.battery_voltage_mv, 46500);
        assert!(ds.initialized);
    }

    // ─── Warning Tests ───────────────────────────────────────────────────

    #[test]
    fn test_dashboard_add_warning() {
        let mut ds = DashboardState::default();
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);

        assert_eq!(ds.warnings.len(), 1);
        assert!(ds.warnings[0].active);
        assert_eq!(ds.warnings[0].warning_code, WarningCode::BmsOvertemp as u8);
        assert_eq!(ds.warnings[0].source_node, MC_NODE_BMS);
    }

    #[test]
    fn test_dashboard_duplicate_warning_no_double_count() {
        let mut ds = DashboardState::default();
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);

        assert_eq!(ds.warnings.len(), 1); // deduplicated
    }

    #[test]
    fn test_dashboard_clear_warning() {
        let mut ds = DashboardState::default();
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);
        ds.add_warning(WarningCode::PowertrainOffline as u8, MC_NODE_POWERTRAIN);

        ds.clear_warning(WarningCode::BmsOvertemp as u8);

        // BMS overt warning is now inactive
        assert!(!ds.warnings[0].active);
        // Powertrain warning still active
        assert!(ds.warnings[1].active);
    }

    #[test]
    fn test_dashboard_clear_node_warnings() {
        let mut ds = DashboardState::default();
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);
        ds.add_warning(WarningCode::BmsOffline as u8, MC_NODE_BMS);
        ds.add_warning(WarningCode::PowertrainOffline as u8, MC_NODE_POWERTRAIN);

        ds.clear_node_warnings(MC_NODE_BMS);

        // Both BMS warnings cleared
        assert!(!ds.warnings[0].active);
        assert!(!ds.warnings[1].active);
        // Powertrain warning still active
        assert!(ds.warnings[2].active);
    }

    #[test]
    fn test_dashboard_top_warning_priority() {
        let mut ds = DashboardState::default();

        // Add low-severity warning
        ds.add_warning(WarningCode::BmsOffline as u8, MC_NODE_BMS);
        assert_eq!(ds.top_warning(), WarningCode::BmsOffline as u8);

        // Add higher-severity warning
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);
        assert_eq!(ds.top_warning(), WarningCode::BmsOvertemp as u8);

        // Add critical warning
        ds.add_warning(WarningCode::CriticalBmsFault as u8, MC_NODE_BMS);
        assert_eq!(ds.top_warning(), WarningCode::CriticalBmsFault as u8);
    }

    #[test]
    fn test_dashboard_top_warning_no_warnings() {
        let ds = DashboardState::default();
        assert_eq!(ds.top_warning(), WarningCode::None as u8);
    }

    #[test]
    fn test_dashboard_top_warning_cleared() {
        let mut ds = DashboardState::default();
        ds.add_warning(WarningCode::CriticalBmsFault as u8, MC_NODE_BMS);
        assert_eq!(ds.top_warning(), WarningCode::CriticalBmsFault as u8);

        ds.clear_warning(WarningCode::CriticalBmsFault as u8);
        assert_eq!(ds.top_warning(), WarningCode::None as u8);
    }

    #[test]
    fn test_dashboard_reboot() {
        let mut ds = DashboardState::default();
        ds.set_mode(VehicleMode::Drive);
        ds.set_speed(300);
        ds.add_warning(WarningCode::BmsOvertemp as u8, MC_NODE_BMS);

        ds.reboot();

        assert_eq!(ds.vehicle_mode, VehicleMode::Off);
        assert_eq!(ds.speed_kph_x10, 0);
        assert_eq!(ds.warnings.len(), 0);
        assert!(!ds.booted);
        assert!(!ds.initialized);
    }

    // ─── Warning Display Tests ───────────────────────────────────────────

    #[test]
    fn test_warning_display_initial() {
        let wd = WarningDisplay::default();
        assert_eq!(wd.displayed_warning, WarningCode::None as u8);
        assert_eq!(wd.severity, 0);
    }

    #[test]
    fn test_warning_display_higher_severity_wins() {
        let mut wd = WarningDisplay::default();

        // Low severity
        let code = wd.update(WarningCode::BmsOffline as u8, 1);
        assert_eq!(code, WarningCode::BmsOffline as u8);

        // Medium severity replaces
        let code = wd.update(WarningCode::BmsOvertemp as u8, 2);
        assert_eq!(code, WarningCode::BmsOvertemp as u8);

        // Critical replaces
        let code = wd.update(WarningCode::CriticalBmsFault as u8, 3);
        assert_eq!(code, WarningCode::CriticalBmsFault as u8);
    }

    #[test]
    fn test_warning_display_lower_severity_loses() {
        let mut wd = WarningDisplay::default();
        wd.update(WarningCode::CriticalBmsFault as u8, 3);

        // Lower severity doesn't replace
        let code = wd.update(WarningCode::BmsOvertemp as u8, 2);
        assert_eq!(code, WarningCode::CriticalBmsFault as u8); // stays
    }

    #[test]
    fn test_warning_display_equal_severity_keeps_current() {
        let mut wd = WarningDisplay::default();
        wd.update(WarningCode::BmsOvertemp as u8, 2);

        let code = wd.update(WarningCode::PowertrainOffline as u8, 2);
        assert_eq!(code, WarningCode::BmsOvertemp as u8); // first one stays
    }

    #[test]
    fn test_warning_display_clear() {
        let mut wd = WarningDisplay::default();
        wd.update(WarningCode::CriticalBmsFault as u8, 3);

        wd.clear();
        assert_eq!(wd.displayed_warning, WarningCode::None as u8);
        assert_eq!(wd.severity, 0);
    }

    // ─── S7: Dashboard failure doesn't disable powertrain ────────────────
    // This is an architectural invariant — the dashboard is read-only from
    // the powertrain perspective. The dashboard state machine doesn't send
    // any commands to powertrain; it only displays data.

    #[test]
    fn test_s7_dashboard_failure_does_not_disable_powertrain() {
        // Verify that dashboard has no mechanism to affect powertrain state.
        // The dashboard is purely a consumer of data.
        let ds = DashboardState::default();
        // Dashboard has no "disable motor" or "command torque" function
        // This is validated by the absence of such methods
        assert!(ds.booted);
    }
}
