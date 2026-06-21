//! Plant model implementing [`EnvironmentModel`] — the bridge between
//! the microcar vehicle plant and the costar simulation World.
//!
//! # CAN protocol
//!
//! | ID     | Name                 | Publisher | Format                              |
//! |--------|----------------------|-----------|-------------------------------------|
//! | 0x102  | MC_MSG_WHEEL_SPEED   | plant     | [speed_hi, speed_lo] (u16 BE)       |
//! | 0x500  | MC_MSG_PLANT_SENSORS | plant     | [soc, volt_hi, volt_lo, temp_hi,    |
//! |        |                      |           |  temp_lo, current_hi, current_lo]   |
//! | 0x100  | MC_MSG_MOTOR_COMMAND | powertrain| [torque_i8, 0, 0, 0, 0]            |
//!
//! Motor commands are read via the driver-input queue (not from bus frames),
//! since firmware ECUs aren't running in the MVP.  The plant publishes
//! wheel speed and sensor readings each tick so other ECUs can receive them
//! when firmware is loaded.

use sim_world::plant::EnvironmentModel;
use sim_world::World;

use crate::battery::BatteryModel;
use crate::sensors::SensorReadings;
use crate::vehicle::VehiclePlant;

/// CAN frame ID for wheel speed messages (published by plant).
/// Matches MC_MSG_WHEEL_SPEED (0x102) in the microcar protocol.
pub const CAN_ID_WHEEL_SPEED: u32 = 0x102;

/// CAN frame ID for plant sensor readings (published by plant).
/// Non-conflicting with BMS ECU's MC_MSG_BMS_STATUS (0x200) and
/// dashboard's MC_MSG_DASHBOARD_STATUS (0x300).
pub const CAN_ID_PLANT_SENSORS: u32 = 0x500;

/// CAN frame ID for motor torque command (read by plant from bus-inject).
pub const CAN_ID_MOTOR_COMMAND: u32 = 0x100;

/// A pending driver input scheduled for a specific virtual time.
#[derive(Debug, Clone)]
struct PendingInput {
    /// Virtual time (ticks) when this input takes effect.
    at: u64,
    /// Throttle position in percent (0-100).
    throttle_percent: u8,
    /// Whether the brake pedal is pressed.
    brake_pressed: bool,
}

/// The full microcar plant model — vehicle dynamics, battery, and sensors.
pub struct MicrocarPlant {
    /// Vehicle dynamics model (speed, torque).
    vehicle: VehiclePlant,

    /// Battery model (SOC, temperature, voltage, current).
    battery: BatteryModel,

    /// Pending driver inputs, sorted by time.
    pending_inputs: Vec<PendingInput>,

    /// Last applied input index (cursor into pending_inputs).
    input_cursor: usize,

    /// Tick duration in milliseconds for the plant step.
    tick_ms: u32,

    /// Machine ID to use as "sender" when publishing CAN frames.
    /// 0 means the plant is an anonymous publisher.
    plant_machine_id: u64,

    /// Name of the CAN bus to publish on.
    bus_name: String,
}

impl MicrocarPlant {
    /// Create a new microcar plant model.
    ///
    /// `tick_ms` is the plant step interval in milliseconds (from the
    /// scenario's `[plant].tick_ms`).
    pub fn new(tick_ms: u32) -> Self {
        Self {
            vehicle: VehiclePlant::new(),
            battery: BatteryModel::new(),
            pending_inputs: Vec::new(),
            input_cursor: 0,
            tick_ms,
            plant_machine_id: 0,
            bus_name: "vcan0".to_string(),
        }
    }

    /// Set the machine ID used as the CAN frame sender.
    pub fn with_machine_id(mut self, id: u64) -> Self {
        self.plant_machine_id = id;
        self
    }

    /// Set the CAN bus name.
    pub fn with_bus(mut self, name: &str) -> Self {
        self.bus_name = name.to_string();
        self
    }

    /// Apply any due driver inputs and advance the vehicle + battery models
    /// by one tick.
    fn do_step(&mut self, now: u64, world: &mut World) {
        // ── Apply pending driver inputs ──────────────────────────
        while self.input_cursor < self.pending_inputs.len()
            && self.pending_inputs[self.input_cursor].at <= now
        {
            let input = &self.pending_inputs[self.input_cursor];
            self.vehicle
                .set_driver_input(input.throttle_percent, input.brake_pressed);
            // Map throttle to motor torque for now (1:1 in demo).
            self.vehicle
                .set_motor_torque(input.throttle_percent as i8);
            self.input_cursor += 1;
        }

        // ── Advance vehicle dynamics ─────────────────────────────
        self.vehicle.step(self.tick_ms);

        // ── Advance battery model ────────────────────────────────
        self.battery
            .step(self.vehicle.motor_torque_percent, self.tick_ms);

        // ── Publish sensor readings onto CAN bus ─────────────────
        let readings = SensorReadings::from_plant(&self.vehicle, &self.battery);

        // Wheel speed: 0x102, 2 bytes (u16 BE, in 0.1 km/h)
        let speed = readings.wheel_speed_kph_x10;
        let speed_data = speed.to_be_bytes().to_vec();
        world.inject_can_frame(
            &self.bus_name,
            self.plant_machine_id,
            CAN_ID_WHEEL_SPEED,
            &speed_data,
            now,
        );

        // Plant sensor readings: 0x500, 7 bytes
        // [soc, volt_hi, volt_lo, temp_hi, temp_lo, current_hi, current_lo]
        let voltage = readings.pack_voltage_mv;
        let temp = readings.pack_temp_c_x10;
        let current = readings.pack_current_ma;
        let mut bms_data = Vec::with_capacity(7);
        bms_data.push(readings.soc_percent);
        bms_data.extend_from_slice(&voltage.to_be_bytes());
        bms_data.extend_from_slice(&temp.to_be_bytes());
        bms_data.extend_from_slice(&current.to_be_bytes());
        world.inject_can_frame(
            &self.bus_name,
            self.plant_machine_id,
            CAN_ID_PLANT_SENSORS,
            &bms_data,
            now,
        );
    }
}

impl EnvironmentModel for MicrocarPlant {
    fn step(&mut self, now: u64, world: &mut World) {
        self.do_step(now, world);
    }

    fn queue_driver_input(&mut self, at: u64, throttle_percent: u8, brake_pressed: bool) {
        self.pending_inputs.push(PendingInput {
            at,
            throttle_percent,
            brake_pressed,
        });
        // Keep sorted by time (inputs should arrive in order from the scenario).
        self.pending_inputs.sort_by_key(|i| i.at);
    }

    fn apply_fault(
        &mut self,
        target: &str,
        fault_type: &str,
        value: Option<u32>,
    ) -> bool {
        if target == "battery" && fault_type == "force_temperature" {
            if let Some(temp_c) = value {
                self.battery.force_temperature(temp_c as f32);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_world::CanBus;
    use sim_world::Machine;

    #[test]
    fn test_plant_publishes_speed_and_bms() {
        // Create a minimal world with a bus.
        let mut world = World::new();
        let mut bus = CanBus::new("vcan0", 500);
        bus.attach(1);
        bus.attach(2);
        world.add_bus(bus);

        let m = Machine::with_defaults(1, "dashboard");
        world.add_machine(m);

        // Create plant and step it at t=0 — no driver input, so speed=0.
        let mut plant = MicrocarPlant::new(10).with_machine_id(99);
        plant.step(0, &mut world);

        // Bus should now have 2 pending frames (wheel speed + BMS) for each
        // attached node (2 nodes → 4 frames, since sender=99 is not attached).
        assert_eq!(world.buses()[0].pending_count(), 4); // 2 nodes × 2 frames

        // Drain at arrival time (500 latency).
        let frames = {
            let bus = world.bus_mut("vcan0").unwrap();
            bus.drain_arrived(500)
        };
        assert_eq!(frames.len(), 4); // 2 nodes × 2 frames

        // First frame should be wheel speed at 0.
        let wheel_frames: Vec<_> = frames.iter().filter(|(_, _, id, _)| *id == CAN_ID_WHEEL_SPEED).collect();
        assert_eq!(wheel_frames.len(), 2); // one per node
        let (_rx, _sender, _id, data) = wheel_frames[0];
        assert_eq!(data.len(), 2);
        // speed = 0 at rest
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 0);

        // Plant sensor frames.
        let bms_frames: Vec<_> = frames.iter().filter(|(_, _, id, _)| *id == CAN_ID_PLANT_SENSORS).collect();
        assert_eq!(bms_frames.len(), 2);
        let (_rx, _sender, _id, data) = bms_frames[0];
        assert_eq!(data.len(), 7);
        // SOC = 80% initially
        assert_eq!(data[0], 80);
    }

    #[test]
    fn test_driver_input_increases_speed() {
        let mut world = World::new();
        let mut bus = CanBus::new("vcan0", 500);
        bus.attach(1);
        world.add_bus(bus);

        let mut plant = MicrocarPlant::new(10).with_machine_id(99);

        // Queue a driver input at t=0.
        plant.queue_driver_input(0, 30, false);

        // Step for 1 second (100 ticks at 10ms each).
        for _ in 0..100 {
            let now = 0; // all at t=0 for simplicity
            plant.step(now, &mut world);
        }

        // Speed should be > 0.
        assert!(
            plant.vehicle.speed_kph_x10 > 0,
            "speed should increase with throttle"
        );

        // Battery SOC should have decreased slightly.
        assert!(
            plant.battery.soc_percent <= 80,
            "SOC should decrease under load"
        );
    }

    #[test]
    fn test_brake_overrides_throttle() {
        let mut world = World::new();
        let mut bus = CanBus::new("vcan0", 500);
        bus.attach(1);
        world.add_bus(bus);

        let mut plant = MicrocarPlant::new(10).with_machine_id(99);
        plant.queue_driver_input(0, 80, true); // brake pressed

        for _ in 0..50 {
            plant.step(0, &mut world);
        }

        assert_eq!(plant.vehicle.speed_kph_x10, 0);
    }

    #[test]
    fn test_multiple_inputs() {
        let mut world = World::new();
        let mut bus = CanBus::new("vcan0", 500);
        bus.attach(1);
        world.add_bus(bus);

        let mut plant = MicrocarPlant::new(10).with_machine_id(99);

        // Inputs at different times.
        plant.queue_driver_input(0, 50, false);    // start at 50%
        plant.queue_driver_input(500_000, 100, false); // increase to 100%

        // Step until second input applies.
        for _ in 0..100 {
            plant.step(500_000, &mut world);
        }

        // After first input (50% throttle), should have some speed.
        let speed_after_50 = plant.vehicle.speed_kph_x10;
        assert!(speed_after_50 > 0);

        // Motor torque should now be 100% (from the last input).
        assert_eq!(plant.vehicle.motor_torque_percent, 100);
        assert_eq!(plant.vehicle.throttle_percent, 100);
    }
}
