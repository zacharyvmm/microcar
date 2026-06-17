//! microcar-state-tests — pure state machine logic tests.
//!
//! This crate validates the state machine logic that will be implemented
//! in the firmware C files. It does not depend on costar or any hardware;
//! it tests the behavioral specification directly.

pub mod bms;
pub mod dashboard;
pub mod gateway;
pub mod powertrain;
pub mod protocol;
