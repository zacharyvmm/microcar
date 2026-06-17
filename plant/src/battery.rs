//! Battery model — SOC, temperature, current draw.

/// Simple battery model with state of charge, temperature, and current.
///
/// Uses i64 internal accumulation for precision.  SOC tracked in milli-percent
/// (0-100000), temperature in 0.1°C units.
#[derive(Debug, Clone)]
pub struct BatteryModel {
    /// State of charge (0-100%).
    pub soc_percent: u8,

    /// Internal SOC in milli-percent for precision (0-100000).
    soc_milli: i64,

    /// Pack temperature in 0.1°C (e.g., 250 = 25.0°C).
    pub temp_c_x10: i16,

    /// Pack voltage in millivolts.
    pub voltage_mv: u16,

    /// Pack current in milliamps (positive = discharge).
    pub current_ma: i16,

    /// Pre-computed nominal voltage (~48V for small EV).
    nominal_voltage_mv: u16,
}

impl Default for BatteryModel {
    fn default() -> Self {
        Self {
            soc_percent: 80,
            soc_milli: 80_000,
            temp_c_x10: 250, // 25.0°C ambient
            voltage_mv: 48000,
            current_ma: 0,
            nominal_voltage_mv: 48000,
        }
    }
}

impl BatteryModel {
    /// Create a new battery at default state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a battery with custom initial state.
    pub fn with_state(soc_percent: u8, temp_c_x10: i16) -> Self {
        let soc_milli = (soc_percent as i64) * 1000;
        let mut b = Self {
            soc_percent,
            soc_milli,
            temp_c_x10,
            ..Self::default()
        };
        b.voltage_mv = b.calc_voltage();
        b
    }

    /// Advance the battery model one tick.
    ///
    /// `torque_percent` is the current motor torque command (used to
    /// estimate current draw).  `dt_ms` is tick duration.
    pub fn step(&mut self, torque_percent: i8, dt_ms: u32) {
        let dt = dt_ms as i64;

        // Current draw proportional to absolute torque (simplified).
        // At 100% torque: ~50A = 50000mA.  Scale linearly.
        let abs_torque = (torque_percent as i32).unsigned_abs() as i64;
        let current_ma_i64 = abs_torque * 500; // 0.5A per torque percent
        self.current_ma = current_ma_i64 as i16;

        // ── SOC: milli-percent precision ──────────────────────────
        // capacity = 50000 mAh = 50 Ah
        // Charge drawn per tick: current_mA * dt_ms / 3600000 (amp-hours)
        // SOC drop = charge_drawn / capacity * 100_000 milli-percent
        // = current_mA * dt_ms * 100_000 / (3_600_000 * 50_000)
        // = current_mA * dt_ms / (3_600_000 * 50_000 / 100_000)
        // = current_mA * dt_ms / 1_800_000
        let capacity_mah: i64 = 50_000;
        let soc_drop_milli = current_ma_i64 * dt * 1000 / (3600 * capacity_mah);
        self.soc_milli = self.soc_milli.saturating_sub(soc_drop_milli).max(0);
        self.soc_percent = ((self.soc_milli + 500) / 1000).min(100) as u8;

        // ── Temperature: heating from I²R ────────────────────────
        // I in amps = current_mA / 1000
        // heat ∝ I² * dt
        // Scale: at 50A (50000mA), I=50, I²=2500.
        // In 10ms: heat = 2500 * 10 / scale
        // We want about +1°C (= +10 units in 0.1°C) in 1 second at 50A.
        // 1s = 100 * 10ms ticks, so per tick ≈ 0.1°C → 1 unit/10ms
        // heat_rise = I² * dt / 25000 → 2500 * 10 / 25000 = 1 unit/tick
        let i_amps = current_ma_i64 / 1000;
        let heat_rise = i_amps * i_amps * dt / 25000;

        // Passive cooling: proportional to temp above ambient (25°C = 250)
        // Cool at ~0.1°C per second per 10°C above ambient
        let above_ambient = (self.temp_c_x10 as i64 - 250).max(0);
        let cooling = above_ambient * dt / 100000;

        let new_temp = (self.temp_c_x10 as i64)
            .saturating_add(heat_rise)
            .saturating_sub(cooling)
            .max(200); // floor at 20°C

        self.temp_c_x10 = new_temp as i16;

        // Voltage sags with current draw; also drops with SOC
        self.voltage_mv = self.calc_voltage();
    }

    /// Force the battery temperature to a specific value (fault injection).
    pub fn force_temperature(&mut self, temp_c: f32) {
        self.temp_c_x10 = (temp_c * 10.0) as i16;
    }

    /// Calculate pack voltage from SOC and current (simplified).
    fn calc_voltage(&self) -> u16 {
        let soc_factor = self.soc_percent as u32;
        let sag = (self.current_ma.unsigned_abs() as u32) / 10; // 100mV per 10A
        let v = (self.nominal_voltage_mv as u32) * soc_factor / 100;
        v.saturating_sub(sag) as u16
    }

    /// Get temperature in degrees Celsius (as f32 for display).
    pub fn temp_c(&self) -> f32 {
        f32::from(self.temp_c_x10) / 10.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soc_decreases_under_load() {
        let mut bat = BatteryModel::new();
        let initial_soc = bat.soc_percent;

        // Run at high torque for 10 seconds of virtual time (1000 * 10ms)
        for _ in 0..1000 {
            bat.step(80, 10);
        }

        assert!(
            bat.soc_percent < initial_soc,
            "SOC should decrease under load (initial={initial_soc}, final={})",
            bat.soc_percent
        );
    }

    #[test]
    fn test_temperature_rises_under_load() {
        let mut bat = BatteryModel::new();
        let initial_temp = bat.temp_c_x10;

        // 100% torque for 10 seconds → should heat up
        for _ in 0..1000 {
            bat.step(100, 10);
        }

        assert!(
            bat.temp_c_x10 > initial_temp,
            "temp should rise under heavy load (initial={initial_temp}, final={})",
            bat.temp_c_x10
        );
    }

    #[test]
    fn test_force_temperature() {
        let mut bat = BatteryModel::new();
        bat.force_temperature(82.0);
        assert_eq!(bat.temp_c_x10, 820);
    }

    #[test]
    fn test_no_load_no_temp_rise() {
        let mut bat = BatteryModel::with_state(80, 250);
        let initial_temp = bat.temp_c_x10;

        // 1 second of zero torque — cooling only
        for _ in 0..100 {
            bat.step(0, 10);
        }

        // At ambient temp, cooling is zero, so temp stays at 25°C
        assert_eq!(bat.temp_c_x10, 250, "temp at ambient should not change");
    }
}
