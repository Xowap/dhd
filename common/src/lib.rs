//! Shared protocol definitions for the DHD (Dial Hifi Device).
//!
//! This crate provides the message types used for communication between the
//! DHD firmware and the host application. Communication is performed using
//! JSON over a serial (CDC-ACM) link.

#![no_std]

pub mod pid;

use serde::{Deserialize, Serialize};

/// Messages sent from the device to the host.
#[derive(Serialize, Deserialize, Debug)]
pub enum OutgoingMessage {
    /// Reports a change in the physical dial's position.
    #[serde(rename = "volume")]
    Volume {
        /// Normalized volume value (0.0 to 1.0).
        value: f32,
    },
    /// Sends a diagnostic log message to the host.
    #[serde(rename = "log")]
    Log {
        /// The log level (e.g., "INFO", "WARN", "ERROR").
        level: heapless::String<16>,
        /// The formatted log message.
        message: heapless::String<128>,
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
    StartCalibration,
}
