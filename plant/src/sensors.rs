//! Sensor readings — publishes plant state to the bus.

use crate::battery::BatteryModel;
use crate::vehicle::VehiclePlant;

/// Readings that the plant publishes to the simulated bus each tick.
#[derive(Debug, Clone, Default)]
pub struct SensorReadings {
    /// Wheel speed in 0.1 km/h.
    pub wheel_speed_kph_x10: u16,

    /// Battery pack voltage in mV.
    pub pack_voltage_mv: u16,

    /// Battery pack current in mA.
    pub pack_current_ma: i16,

    /// Battery temperature in 0.1°C.
    pub pack_temp_c_x10: i16,

    /// State of charge percent.
    pub soc_percent: u8,
}

impl SensorReadings {
    /// Gather readings from the vehicle and battery models.
    pub fn from_plant(vehicle: &VehiclePlant, battery: &BatteryModel) -> Self {
        Self {
            wheel_speed_kph_x10: vehicle.speed_kph_x10,
            pack_voltage_mv: battery.voltage_mv,
            pack_current_ma: battery.current_ma,
            pack_temp_c_x10: battery.temp_c_x10,
            soc_percent: battery.soc_percent,
        }
    }
}
