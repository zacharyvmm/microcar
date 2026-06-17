//! gateway.rs — gateway state machine logic (pure, no side effects).
//!
//! Tests the gateway's vehicle mode transitions, heartbeat monitor,
//! and fault aggregation — the same logic implemented in C firmware.

use super::protocol::*;

// ── Gateway State Machine ────────────────────────────────────────────────

/// Gateway internal state — mirrors gateway_state_t in C.
#[derive(Debug, Clone)]
pub struct GatewayState {
    pub mode: VehicleMode,
    pub all_nodes_online: bool,
    pub bms_fault_active: bool,
    pub bms_limp_requested: bool,
    pub active_fault_count: u8,
}

impl Default for GatewayState {
    fn default() -> Self {
        Self {
            mode: VehicleMode::Off,
            all_nodes_online: false,
            bms_fault_active: false,
            bms_limp_requested: false,
            active_fault_count: 0,
        }
    }
}

impl GatewayState {
    /// Update gateway state based on inputs and return the new mode.
    pub fn update(
        &mut self,
        all_nodes_online: bool,
        bms_fault_active: bool,
        bms_limp_requested: bool,
        fault_count: u8,
    ) -> VehicleMode {
        self.all_nodes_online = all_nodes_online;
        self.bms_fault_active = bms_fault_active;
        self.bms_limp_requested = bms_limp_requested;
        self.active_fault_count = fault_count;

        self.mode = determine_mode(
            self.mode,
            all_nodes_online,
            bms_fault_active,
            bms_limp_requested,
        );
        self.mode
    }

    pub fn enter_drive(&mut self) {
        if self.mode == VehicleMode::Ready {
            self.mode = VehicleMode::Drive;
        }
    }

    pub fn reboot(&mut self) {
        *self = Self::default();
    }
}

/// Pure function: determine vehicle mode from current state and inputs.
/// Mirrors mc_gateway_determine_mode() in common/src/microcar_protocol.c.
pub fn determine_mode(
    current: VehicleMode,
    all_nodes_online: bool,
    bms_fault_active: bool,
    bms_limp_requested: bool,
) -> VehicleMode {
    // FAULT is terminal until reboot
    if current == VehicleMode::Fault {
        return VehicleMode::Fault;
    }

    // Critical BMS fault → FAULT (overrides everything)
    if bms_fault_active {
        return VehicleMode::Fault;
    }

    // CHARGING is a special mode — limp request doesn't apply;
    // only critical fault (handled above) can override it.
    if current == VehicleMode::Charging {
        return VehicleMode::Charging;
    }

    // BMS limp request → LIMP (overrides node online status —
    // the BMS IS communicating if it sent a limp request)
    if bms_limp_requested {
        return VehicleMode::Limp;
    }

    // Normal transitions based on node status
    match current {
        VehicleMode::Off => {
            if all_nodes_online {
                VehicleMode::Ready
            } else {
                VehicleMode::Off
            }
        }
        VehicleMode::Ready => {
            if !all_nodes_online {
                VehicleMode::Off
            } else {
                VehicleMode::Ready
            }
        }
        VehicleMode::Drive => {
            if !all_nodes_online {
                VehicleMode::Ready
            } else {
                VehicleMode::Drive
            }
        }
        VehicleMode::Limp => {
            // LIMP without active limp request → recovering
            if all_nodes_online {
                VehicleMode::Ready
            } else {
                VehicleMode::Off
            }
        }
        _ => VehicleMode::Off,
    }
}

// ── Heartbeat Monitor ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HeartbeatMonitor {
    nodes: Vec<HeartbeatNode>,
}

#[derive(Debug, Clone)]
struct HeartbeatNode {
    node_id: u8,
    online: bool,
    last_beat_ms: u32,
    timeout_ms: u32,
}

impl HeartbeatMonitor {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn register(&mut self, node_id: u8, timeout_ms: u32) {
        self.nodes.push(HeartbeatNode {
            node_id,
            online: false,
            last_beat_ms: 0,
            timeout_ms,
        });
    }

    pub fn beat(&mut self, node_id: u8, now_ms: u32) {
        for node in &mut self.nodes {
            if node.node_id == node_id {
                node.last_beat_ms = now_ms;
                node.online = true;
                return;
            }
        }
    }

    pub fn check(&mut self, now_ms: u32) -> usize {
        let mut transitions = 0;
        for node in &mut self.nodes {
            let was_online = node.online;
            if was_online && now_ms - node.last_beat_ms >= node.timeout_ms {
                node.online = false;
                transitions += 1;
            }
        }
        transitions
    }

    pub fn all_online(&self) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        self.nodes.iter().all(|n| n.online)
    }

    pub fn is_online(&self, node_id: u8) -> bool {
        self.nodes.iter().any(|n| n.node_id == node_id && n.online)
    }
}

// ── Fault Manager ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FaultManager {
    faults: Vec<FaultEntry>,
    critical_count: u8,
    warning_count: u8,
}

#[derive(Debug, Clone)]
struct FaultEntry {
    source_node: u8,
    fault_code: u8,
    active: bool,
    severity: u8, // 0=info, 1=warning, 2=critical
}

impl FaultManager {
    pub fn new() -> Self {
        Self {
            faults: Vec::new(),
            critical_count: 0,
            warning_count: 0,
        }
    }

    pub fn report(&mut self, source: u8, code: u8, severity: u8) {
        // Check if already exists
        for f in &mut self.faults {
            if f.source_node == source && f.fault_code == code {
                if !f.active {
                    f.active = true;
                    match severity {
                        2 => self.critical_count += 1,
                        1 => self.warning_count += 1,
                        _ => {}
                    }
                }
                f.severity = severity;
                return;
            }
        }

        // New fault
        self.faults.push(FaultEntry {
            source_node: source,
            fault_code: code,
            active: true,
            severity,
        });
        match severity {
            2 => self.critical_count += 1,
            1 => self.warning_count += 1,
            _ => {}
        }
    }

    pub fn clear(&mut self, source: u8, code: u8) {
        for f in &mut self.faults {
            if f.source_node == source && f.fault_code == code && f.active {
                match f.severity {
                    2 => self.critical_count -= 1,
                    1 => self.warning_count -= 1,
                    _ => {}
                }
                f.active = false;
                return;
            }
        }
    }

    pub fn has_critical(&self) -> bool {
        self.critical_count > 0
    }

    pub fn active_count(&self) -> u8 {
        self.faults.iter().filter(|f| f.active).count() as u8
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Gateway State Machine Tests ─────────────────────────────────────

    #[test]
    fn test_gateway_initial_state_off() {
        let gw = GatewayState::default();
        assert_eq!(gw.mode, VehicleMode::Off);
        assert!(!gw.all_nodes_online);
    }

    #[test]
    fn test_gateway_off_to_ready_when_all_nodes_online() {
        let mut gw = GatewayState::default();
        let mode = gw.update(true, false, false, 0);
        assert_eq!(mode, VehicleMode::Ready);
    }

    #[test]
    fn test_gateway_stays_off_when_nodes_offline() {
        let mut gw = GatewayState::default();
        let mode = gw.update(false, false, false, 0);
        assert_eq!(mode, VehicleMode::Off);
    }

    #[test]
    fn test_gateway_ready_to_drive() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        assert_eq!(gw.mode, VehicleMode::Ready);
        gw.enter_drive();
        assert_eq!(gw.mode, VehicleMode::Drive);
    }

    #[test]
    fn test_gateway_drive_back_to_ready_on_node_loss() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        gw.enter_drive(); // READY → DRIVE
        assert_eq!(gw.mode, VehicleMode::Drive);

        // Node goes offline
        let mode = gw.update(false, false, false, 0);
        assert_eq!(mode, VehicleMode::Ready);
    }

    #[test]
    fn test_gateway_ready_back_to_off_on_node_loss() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        assert_eq!(gw.mode, VehicleMode::Ready);

        let mode = gw.update(false, false, false, 0);
        assert_eq!(mode, VehicleMode::Off);
    }

    #[test]
    fn test_gateway_off_to_limp_on_bms_limp_request() {
        let mut gw = GatewayState::default();
        // Even before all nodes online, BMS limp request takes priority
        let mode = gw.update(false, false, true, 0);
        assert_eq!(mode, VehicleMode::Limp);
    }

    #[test]
    fn test_gateway_ready_to_limp_on_bms_limp_request() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        assert_eq!(gw.mode, VehicleMode::Ready);

        let mode = gw.update(true, false, true, 0);
        assert_eq!(mode, VehicleMode::Limp);
    }

    #[test]
    fn test_gateway_drive_to_limp_on_bms_limp() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0);
        gw.enter_drive();
        assert_eq!(gw.mode, VehicleMode::Drive);

        let mode = gw.update(true, false, true, 0);
        assert_eq!(mode, VehicleMode::Limp);
    }

    #[test]
    fn test_gateway_limp_back_to_ready_when_limp_clears() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        gw.update(true, false, true, 0); // READY → LIMP
        assert_eq!(gw.mode, VehicleMode::Limp);

        // LIMP clears + all nodes online → READY
        let mode = gw.update(true, false, false, 0);
        assert_eq!(mode, VehicleMode::Ready);
    }

    #[test]
    fn test_gateway_limp_persists_on_node_loss() {
        let mut gw = GatewayState::default();
        gw.update(true, false, true, 0); // OFF → LIMP (via limp request)
        assert_eq!(gw.mode, VehicleMode::Limp);

        // Nodes go offline but limp still active — stays in LIMP
        // because BMS is still communicating (the limp request proves it)
        let mode = gw.update(false, false, true, 0);
        assert_eq!(mode, VehicleMode::Limp);
    }

    #[test]
    fn test_gateway_fault_on_critical_bms_fault() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        assert_eq!(gw.mode, VehicleMode::Ready);

        let mode = gw.update(true, true, false, 0); // critical BMS fault
        assert_eq!(mode, VehicleMode::Fault);
    }

    #[test]
    fn test_gateway_fault_is_terminal() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0); // OFF → READY
        gw.update(true, true, false, 0); // READY → FAULT
        assert_eq!(gw.mode, VehicleMode::Fault);

        // Even if all faults clear, FAULT persists
        let mode = gw.update(true, false, false, 0);
        assert_eq!(mode, VehicleMode::Fault);
    }

    #[test]
    fn test_gateway_reboot_resets_to_off() {
        let mut gw = GatewayState::default();
        gw.update(true, false, false, 0);
        gw.enter_drive();
        assert_eq!(gw.mode, VehicleMode::Drive);

        gw.reboot();
        assert_eq!(gw.mode, VehicleMode::Off);
        assert!(!gw.all_nodes_online);
    }

    #[test]
    fn test_gateway_charging_mode() {
        // CHARGING mode is preserved through updates
        let mode = determine_mode(VehicleMode::Charging, true, false, false);
        assert_eq!(mode, VehicleMode::Charging);

        // Even with limp request, CHARGING stays
        let mode = determine_mode(VehicleMode::Charging, true, false, true);
        assert_eq!(mode, VehicleMode::Charging);

        // But critical fault takes priority
        let mode = determine_mode(VehicleMode::Charging, true, true, false);
        assert_eq!(mode, VehicleMode::Fault);
    }

    // ─── Heartbeat Monitor Tests ─────────────────────────────────────────

    #[test]
    fn test_heartbeat_monitor_new_empty() {
        let hm = HeartbeatMonitor::new();
        assert!(!hm.all_online());
    }

    #[test]
    fn test_heartbeat_monitor_register_and_beat() {
        let mut hm = HeartbeatMonitor::new();
        hm.register(MC_NODE_POWERTRAIN, 250);
        hm.register(MC_NODE_BMS, 300);

        assert!(!hm.all_online());
        assert!(!hm.is_online(MC_NODE_POWERTRAIN));

        hm.beat(MC_NODE_POWERTRAIN, 100);
        assert!(hm.is_online(MC_NODE_POWERTRAIN));
        assert!(!hm.is_online(MC_NODE_BMS)); // no beat yet
        assert!(!hm.all_online());

        hm.beat(MC_NODE_BMS, 150);
        assert!(hm.is_online(MC_NODE_BMS));
        assert!(hm.all_online());
    }

    #[test]
    fn test_heartbeat_monitor_timeout() {
        let mut hm = HeartbeatMonitor::new();
        hm.register(MC_NODE_BMS, 300); // 300ms timeout

        // BMS sends heartbeat at t=100
        hm.beat(MC_NODE_BMS, 100);
        assert!(hm.is_online(MC_NODE_BMS));

        // At t=200, still online (100ms elapsed < 300ms)
        let transitions = hm.check(200);
        assert_eq!(transitions, 0);
        assert!(hm.is_online(MC_NODE_BMS));

        // At t=400, timed out (300ms elapsed ≥ 300ms)
        let transitions = hm.check(400);
        assert_eq!(transitions, 1);
        assert!(!hm.is_online(MC_NODE_BMS));
    }

    #[test]
    fn test_heartbeat_monitor_timeout_exact_boundary() {
        let mut hm = HeartbeatMonitor::new();
        hm.register(MC_NODE_GATEWAY, 250);

        hm.beat(MC_NODE_GATEWAY, 100);

        // At t=349: 249ms elapsed, still online
        let transitions = hm.check(349);
        assert_eq!(transitions, 0);

        // At t=350: 250ms elapsed, should timeout
        let transitions = hm.check(350);
        assert_eq!(transitions, 1);
        assert!(!hm.is_online(MC_NODE_GATEWAY));
    }

    #[test]
    fn test_heartbeat_monitor_multiple_nodes() {
        let mut hm = HeartbeatMonitor::new();
        hm.register(MC_NODE_POWERTRAIN, 250);
        hm.register(MC_NODE_BMS, 300);
        hm.register(MC_NODE_DASHBOARD, 500);

        // All beat at t=100
        hm.beat(MC_NODE_POWERTRAIN, 100);
        hm.beat(MC_NODE_BMS, 100);
        hm.beat(MC_NODE_DASHBOARD, 100);
        assert!(hm.all_online());

        // t=350: powertrain timed out (250ms timeout)
        let transitions = hm.check(350);
        assert_eq!(transitions, 1); // only powertrain
        assert!(!hm.is_online(MC_NODE_POWERTRAIN));
        assert!(hm.is_online(MC_NODE_BMS)); // BMS timeout is 300ms, 250ms elapsed
        assert!(hm.is_online(MC_NODE_DASHBOARD));

        // t=410: BMS timed out (310ms > 300ms)
        let transitions = hm.check(410);
        assert_eq!(transitions, 1); // BMS just timed out
        assert!(!hm.is_online(MC_NODE_BMS));
    }

    // ─── Fault Manager Tests ─────────────────────────────────────────────

    #[test]
    fn test_fault_manager_new_has_no_critical() {
        let fm = FaultManager::new();
        assert!(!fm.has_critical());
        assert_eq!(fm.active_count(), 0);
    }

    #[test]
    fn test_fault_manager_report_critical() {
        let mut fm = FaultManager::new();
        fm.report(MC_NODE_BMS, BmsFaultCode::Overtemp as u8, 2);
        assert!(fm.has_critical());
        assert_eq!(fm.active_count(), 1);
    }

    #[test]
    fn test_fault_manager_report_warning() {
        let mut fm = FaultManager::new();
        fm.report(MC_NODE_BMS, BmsFaultCode::Undervoltage as u8, 1);
        assert!(!fm.has_critical());
        assert_eq!(fm.active_count(), 1);
    }

    #[test]
    fn test_fault_manager_clear_fault() {
        let mut fm = FaultManager::new();
        fm.report(MC_NODE_BMS, BmsFaultCode::Overtemp as u8, 2);
        assert!(fm.has_critical());

        fm.clear(MC_NODE_BMS, BmsFaultCode::Overtemp as u8);
        assert!(!fm.has_critical());
        assert_eq!(fm.active_count(), 0);
    }

    #[test]
    fn test_fault_manager_duplicate_report_no_double_count() {
        let mut fm = FaultManager::new();
        fm.report(MC_NODE_BMS, BmsFaultCode::Overtemp as u8, 2);
        fm.report(MC_NODE_BMS, BmsFaultCode::Overtemp as u8, 2);
        assert_eq!(fm.active_count(), 1);
        assert!(fm.has_critical());
    }

    #[test]
    fn test_fault_manager_multiple_sources() {
        let mut fm = FaultManager::new();
        fm.report(MC_NODE_BMS, BmsFaultCode::Overtemp as u8, 2);
        fm.report(MC_NODE_POWERTRAIN, 1, 1); // warning-level powertrain fault
        assert!(fm.has_critical());
        assert_eq!(fm.active_count(), 2);
    }
}
