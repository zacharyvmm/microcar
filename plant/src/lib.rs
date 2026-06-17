//! microcar-plant — deterministic vehicle plant model.
//!
//! Models speed, battery state of charge, battery temperature, and motor
//! temperature.  Receives motor torque command and driver input; publishes
//! wheel speed and battery sensor readings.
//!
//! Intentionally simple and deterministic — no real physics, no floating
//! point non-determinism (all fixed-point or discrete math).

pub mod battery;
pub mod sensors;
pub mod vehicle;

pub use battery::BatteryModel;
pub use sensors::SensorReadings;
pub use vehicle::VehiclePlant;
