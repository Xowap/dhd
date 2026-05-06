//! Shared protocol definitions for the DHD (Dial Hifi Device).
//!
//! This crate provides the message types used for communication between the
//! DHD firmware and the host application. Communication is performed using
//! JSON over a serial (CDC-ACM) link.

#![no_std]

/// Module containing the PID controller implementation.
pub mod pid;

use serde::{Deserialize, Serialize};

/// Current operation mode of the system.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SystemMode {
    /// Initializing system state
    Init = 0,
    /// Calibrating physical and PID boundaries
    Calibration = 1,
    /// Waiting for host or user interaction
    Standby = 2,
    /// Critical failure mode
    Failsafe = 3,
    /// Knob is being physically pushed by the user
    PhysicallyDriven = 4,
    /// Knob is being logically driven by the host
    LogicallyDriven = 5,
}

/// Messages sent from the device to the host.
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Debug)]
pub enum OutgoingMessage {
    /// Reports a change in the physical dial's position.
    #[serde(rename = "volume")]
    Volume {
        /// Normalized volume value (0.0 to 1.0).
        value: f32,
    },
    /// Reports a change in the system mode.
    #[serde(rename = "mode")]
    Mode {
        /// The new system mode.
        mode: SystemMode,
    },
    /// Sends a diagnostic log message to the host.
    #[serde(rename = "log")]
    Log {
        /// The log level (e.g., "INFO", "WARN", "ERROR").
        level: heapless::String<16>,
        /// The formatted log message.
        message: heapless::String<384>,
    },
    /// Responds to a host health check (Ping).
    #[serde(rename = "pong")]
    Pong {
        /// The timestamp received in the original Ping.
        timestamp: u64,
    },
    /// Handshake response for device identification.
    #[serde(rename = "handshake")]
    Handshake {
        /// The response message.
        message: heapless::String<32>,
    },
    /// Reports whether the scale is currently inverted.
    #[serde(rename = "scale_inverted")]
    ScaleInverted {
        /// True if the scale is inverted (hardware 100% = volume 0%).
        inverted: bool,
    },
}

/// Messages received by the device from the host.
#[derive(Serialize, Deserialize, Debug)]
pub enum IncomingMessage {
    /// A health check sent by the host to verify connection status.
    #[serde(rename = "ping")]
    Ping {
        /// An arbitrary timestamp to be echoed back in the Pong.
        timestamp: u64,
    },
    /// Initial handshake to verify device type.
    #[serde(rename = "handshake")]
    Handshake {
        /// The handshake message.
        message: heapless::String<32>,
    },
    /// Updates the potentiometer calibration range.
    #[serde(rename = "update_calibration")]
    UpdateCalibration {
        /// Bottom clipping level (raw counts).
        bottom: u16,
        /// Top clipping level (raw counts).
        top: u16,
    },
    /// Triggers the device's auto-calibration routine.
    #[serde(rename = "start_calibration")]
    StartCalibration {
        /// Initial volume to set after calibration.
        volume: f32,
        /// If true, force a full re-calibration even if data is stored.
        #[serde(default)]
        force: bool,
    },
    /// Sets the volume of the device from the host.
    #[serde(rename = "set_volume")]
    SetVolume {
        /// Normalized volume value (0.0 to 1.0).
        value: f32,
    },
    /// Toggles scale inversion on the device.
    /// When inverted, hardware 100% reads as volume 0% and vice-versa.
    #[serde(rename = "invert_scale")]
    InvertScale,
}
