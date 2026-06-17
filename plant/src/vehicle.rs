//! Vehicle plant model — speed, torque integration, drag.

/// Top-level vehicle plant state.
#[derive(Debug, Clone, Default)]
pub struct VehiclePlant {
    /// Current speed in 0.1 km/h (e.g., 120 = 12.0 km/h).
    pub speed_kph_x10: u16,

    /// Throttle percent (0-100).
    pub throttle_percent: u8,

    /// Brake pressed flag.
    pub brake_pressed: bool,

    /// Motor torque percent (-100..100, negative = regen).
    pub motor_torque_percent: i8,

    /// Charger plugged flag (drive inhibited).
    pub charger_plugged: bool,
}

impl VehiclePlant {
    /// Create a new vehicle plant at rest.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance the plant model by one tick.
    ///
    /// `dt_ms` is the tick duration in milliseconds.  The model uses
    /// integer arithmetic for determinism.
    pub fn step(&mut self, dt_ms: u32) {
        // Brake always wins over throttle.
        let effective_torque = if self.brake_pressed {
            0
        } else {
            self.motor_torque_percent
        };

        // Speed delta = torque * dt * acceleration_factor / scale
        // acceleration_factor = 1 (percent-to-speed scaling)
        // scale = 100 for fixed-point with dt in ms, units in 0.1 km/h
        let delta: i32 = i32::from(effective_torque) * (dt_ms as i32) * 1 / 100;

        // Drag: lose 0.01% of speed per tick (simplified)
        let drag: i32 = i32::from(self.speed_kph_x10) * (dt_ms as i32) / 10000;

        let new_speed = i32::from(self.speed_kph_x10)
            .saturating_add(delta)
            .saturating_sub(drag)
            .max(0);

        self.speed_kph_x10 = new_speed as u16;
    }

    /// Set driver inputs for this tick.
    pub fn set_driver_input(&mut self, throttle_percent: u8, brake_pressed: bool) {
        self.throttle_percent = throttle_percent;
        self.brake_pressed = brake_pressed;
    }

    /// Set motor torque command from powertrain.
    pub fn set_motor_torque(&mut self, torque_percent: i8) {
        self.motor_torque_percent = torque_percent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_increases_with_throttle() {
        let mut plant = VehiclePlant::new();
        plant.set_driver_input(50, false);
        plant.set_motor_torque(30);

        // 1000 ms of 30% torque
        for _ in 0..100 {
            plant.step(10);
        }

        // Speed should have increased
        assert!(plant.speed_kph_x10 > 0, "speed should increase with throttle");
    }

    #[test]
    fn test_brake_overrides_throttle() {
        let mut plant = VehiclePlant::new();
        plant.set_driver_input(80, true); // brake pressed
        plant.set_motor_torque(50);

        plant.step(10);

        // Even with 50% torque, brake prevents acceleration
        // Speed should stay at 0 (drag can't go negative)
        assert_eq!(plant.speed_kph_x10, 0);
    }

    #[test]
    fn test_zero_torque_no_acceleration() {
        let mut plant = VehiclePlant::new();
        plant.set_motor_torque(0);
        plant.step(100);
        assert_eq!(plant.speed_kph_x10, 0);
    }

    #[test]
    fn test_drag_slows_vehicle() {
        let mut plant = VehiclePlant::new();
        // Set initial speed by simulating torque
        plant.set_motor_torque(50);
        for _ in 0..200 {
            plant.step(10); // 2000ms of 50% torque
        }
        let speed_under_torque = plant.speed_kph_x10;

        // Remove torque, let drag work
        plant.set_motor_torque(0);
        for _ in 0..100 {
            plant.step(10); // 1000ms of drag
        }
        assert!(plant.speed_kph_x10 < speed_under_torque, "drag should slow vehicle");
    }

    #[test]
    fn test_speed_cant_go_negative() {
        let mut plant = VehiclePlant::new();
        // At rest with no torque
        plant.step(10);
        assert_eq!(plant.speed_kph_x10, 0);
    }
}
