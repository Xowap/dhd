//! DHD (Dial Hifi Device) Host Tool
//!
//! This application runs on the host computer and communicates with the DHD
//! device over a virtual serial port (CDC-ACM). It provides:
//! - Automatic device discovery and reconnection.
//! - Real-time volume monitoring with visual feedback.
//! - Diagnostic log display from the device.
//! - Health monitoring via periodic pings.

use anyhow::{Context, Result};
use chrono::Local;
use colored::*;
use common::{IncomingMessage, OutgoingMessage};
use serialport::SerialPort;
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

/// Raspberry Pi Pico Vendor ID.
const VID: u16 = 0x2e8a;
/// Raspberry Pi Pico CDC-ACM Product ID.
const PID: u16 = 0x000a;

fn main() -> Result<()> {
    println!("{}", "=== DHD Host Starting ===".bold().cyan());

    loop {
        match find_and_connect() {
            Ok(mut port) => {
                println!("{}", "Connected to DHD device!".green());
                if let Err(e) = run_host(&mut port) {
                    eprintln!("{} {}", "Connection lost:".red(), e);
                }
            }
            Err(_) => {
                // Silently retry to find the device
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

/// Scans available USB serial ports for a DHD device and opens it.
fn find_and_connect() -> Result<Box<dyn SerialPort>> {
    let ports = serialport::available_ports().context("Failed to list serial ports")?;
    for p in ports {
        if let serialport::SerialPortType::UsbPort(info) = p.port_type {
            if info.vid == VID && info.pid == PID {
                let port = serialport::new(p.port_name, 115_200)
                    .timeout(Duration::from_millis(100))
                    .open()
                    .context("Failed to open serial port")?;
                return Ok(port);
            }
        }
    }
    anyhow::bail!("Device not found")
}

/// Main communication loop for an active connection.
///
/// Handles sending periodic Pings and processing incoming JSON messages from the device.
fn run_host(port: &mut Box<dyn SerialPort>) -> Result<()> {
    let mut reader = BufReader::new(port.try_clone()?);
    let mut last_ping = Instant::now();
    let mut ping_timestamp: u64 = 0;

    loop {
        // Send Ping every 2 seconds to check device health
        if last_ping.elapsed() >= Duration::from_secs(2) {
            ping_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64;
            let ping = IncomingMessage::Ping {
                timestamp: ping_timestamp,
            };
            let j = serde_json::to_string(&ping)? + "\n";
            port.write_all(j.as_bytes())?;
            last_ping = Instant::now();
        }

        // Read line from device
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return Err(anyhow::anyhow!("EOF reached")),
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                match serde_json::from_str::<OutgoingMessage>(line) {
                    Ok(msg) => handle_message(msg, ping_timestamp),
                    Err(e) => {
                        // Ignore non-json for now (might be partial or garbage during connection)
                        if line.contains('{') {
                            eprintln!(
                                "{} {} (line: {})",
                                "Failed to parse JSON:".yellow(),
                                e,
                                line
                            );
                        }
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // Normal, keep looping to send pings
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Dispatches an incoming `OutgoingMessage` to the appropriate display logic.
fn handle_message(msg: OutgoingMessage, last_ping_ts: u64) {
    let now = Local::now().format("%H:%M:%S%.3f").to_string().dimmed();
    match msg {
        OutgoingMessage::Volume { value } => {
            let bar_len = (value * 20.0).clamp(0.0, 20.0) as usize;
            let bar = "|".repeat(bar_len) + &"-".repeat(20 - bar_len);
            println!(
                "{} [{}] {} {:.3}",
                now,
                "VOL".blue().bold(),
                bar.blue(),
                value
            );
        }
        OutgoingMessage::Log { level, message } => {
            let lvl = match level.as_str() {
                "INFO" => "INFO".green(),
                "WARN" => "WARN".yellow(),
                "ERROR" => "ERROR".red(),
                _ => level.as_str().normal(),
            };
            println!("{} [{}] [{}] {}", now, "LOG".white().bold(), lvl, message);
        }
        OutgoingMessage::Pong { timestamp } => {
            if timestamp == last_ping_ts {
                println!("{} [{}] healthy", now, "HEALTH".cyan().bold());
            }
        }
    }
}
