//! DHD (Dial Hifi Device) Host Tool
//!
//! This application runs on the host computer and communicates with the DHD
//! device over a virtual serial port (CDC-ACM). It provides:
//! - Automatic device discovery and reconnection.
//! - Real-time volume monitoring with visual feedback.
//! - Diagnostic log display from the device.
//! - Health monitoring via periodic pings.

use anyhow::{Context as _, Result};
use chrono::Local;
use colored::*;
use common::{IncomingMessage, OutgoingMessage};
use futures::{SinkExt, StreamExt};
use libpulse_binding as pulse;
use pulse::context::{Context, State};
use pulse::context::subscribe::Facility;
use pulse::mainloop::threaded::Mainloop;
use pulse::volume::{Volume, ChannelVolumes};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::interval;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tokio_util::codec::{Framed, LinesCodec};

/// Raspberry Pi Pico Vendor ID.
const VID: u16 = 0x2e8a;
/// Raspberry Pi Pico CDC-ACM Product ID.
const PID: u16 = 0x000a;

#[derive(Default)]
struct PulseCache {
    sink_index: Option<u32>,
    num_channels: u8,
    needs_refresh: bool,
}

/// PulseAudio controller for system volume adjustment.
/// 
/// Uses a threaded mainloop to handle PulseAudio events and callbacks
/// asynchronously from the main Tokio loop.
struct PulseController {
    mainloop: Mainloop,
    context: Context,
    cache: Arc<Mutex<PulseCache>>,
}

// PulseController is safe to share across threads because we use the
// ThreadedMainloop's locking mechanism to synchronize access to the context.
unsafe impl Send for PulseController {}
unsafe impl Sync for PulseController {}

impl PulseController {
    fn new() -> Result<Self> {
        let mut mainloop = Mainloop::new()
            .ok_or_else(|| anyhow::anyhow!("Failed to create PulseAudio mainloop"))?;
        
        let mut context = Context::new(&mainloop, "DHD Host")
            .ok_or_else(|| anyhow::anyhow!("Failed to create PulseAudio context"))?;

        context.connect(None, pulse::context::FlagSet::NOFLAGS, None)
            .map_err(|e| anyhow::anyhow!("Failed to connect PulseAudio context: {:?}", e))?;

        mainloop.start().map_err(|e| anyhow::anyhow!("Failed to start PulseAudio mainloop: {:?}", e))?;

        // Wait for context to be ready
        loop {
            mainloop.lock();
            let state = context.get_state();
            mainloop.unlock();

            match state {
                State::Ready => break,
                State::Failed | State::Terminated => {
                    anyhow::bail!("PulseAudio context failed or terminated");
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }

        let cache = Arc::new(Mutex::new(PulseCache {
            needs_refresh: true,
            ..Default::default()
        }));
        let cache_clone = Arc::clone(&cache);

        mainloop.lock();
        context.set_subscribe_callback(Some(Box::new(move |facility, _op, _index| {
            if let Some(Facility::Server) = facility {
                if let Ok(mut c) = cache_clone.lock() {
                    if !c.needs_refresh {
                        let now = Local::now().format("%H:%M:%S%.3f").to_string().dimmed();
                        println!("{} [{}] Default sink change detected", now, "PULSE".magenta().bold());
                        c.needs_refresh = true;
                    }
                }
            }
        })));

        context.subscribe(
            pulse::context::subscribe::InterestMaskSet::SERVER,
            |_| {}
        );
        mainloop.unlock();

        Ok(Self { 
            mainloop, 
            context,
            cache,
        })
    }

    fn set_volume(&mut self, value: f32) {
        // Map 0.0..1.0 to 0..Volume::NORMAL (100% in most UIs like KDE)
        let vol = Volume((Volume::NORMAL.0 as f32 * value) as u32);
        
        self.mainloop.lock();
        
        let (index, n_channels, needs_refresh) = {
            let c = self.cache.lock().unwrap();
            (c.sink_index, c.num_channels, c.needs_refresh)
        };
        
        if needs_refresh || index.is_none() {
            let ctx_ptr = &mut self.context as *mut Context;
            let cache_ptr = Arc::clone(&self.cache);
            
            self.context.introspect().get_sink_info_by_name("@DEFAULT_SINK@", move |res| {
                if let pulse::callbacks::ListResult::Item(info) = res {
                    let now = Local::now().format("%H:%M:%S%.3f").to_string().dimmed();
                    let name = info.name.as_deref().unwrap_or("unknown");
                    let desc = info.description.as_deref().unwrap_or("no description");
                    println!(
                        "{} [{}] Default sink: {} ({})",
                        now,
                        "PULSE".magenta().bold(),
                        name.cyan(),
                        desc.italic().dimmed()
                    );
                    
                    if let Ok(mut c) = cache_ptr.lock() {
                        c.sink_index = Some(info.index);
                        c.num_channels = info.volume.get().len() as u8;
                        c.needs_refresh = false;
                    }

                    let mut new_volume = info.volume;
                    for v in new_volume.get_mut() {
                        *v = vol;
                    }
                    unsafe {
                        (*ctx_ptr).introspect().set_sink_volume_by_index(
                            info.index,
                            &new_volume,
                            None,
                        );
                    }
                }
            });
        } else {
            let mut cv = ChannelVolumes::default();
            cv.set(n_channels, vol);
            self.context.introspect().set_sink_volume_by_index(
                index.unwrap(),
                &cv,
                None,
            );
        }
        
        self.mainloop.unlock();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "=== DHD Host Starting ===".bold().cyan());

    let pulse = match PulseController::new() {
        Ok(p) => Some(Arc::new(Mutex::new(p))),
        Err(e) => {
            eprintln!(
                "{} {}",
                "Warning: PulseAudio connection failed:".yellow(),
                e
            );
            None
        }
    };

    loop {
        match find_and_connect() {
            Ok(stream) => {
                println!("{}", "Connected to DHD device!".green());
                let framed = Framed::new(stream, LinesCodec::new());
                if let Err(e) = run_host(framed, pulse.clone()).await {
                    eprintln!("{} {}", "Connection lost:".red(), e);
                }
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Scans available USB serial ports for a DHD device and opens it.
fn find_and_connect() -> Result<SerialStream> {
    let ports = serialport::available_ports().context("Failed to list serial ports")?;
    for p in ports {
        if let serialport::SerialPortType::UsbPort(info) = p.port_type {
            if info.vid == VID && info.pid == PID {
                let stream = tokio_serial::new(p.port_name, 115_200)
                    .open_native_async()
                    .context("Failed to open serial port")?;
                return Ok(stream);
            }
        }
    }
    anyhow::bail!("Device not found")
}

/// Main communication loop for an active connection.
async fn run_host(
    mut framed: Framed<SerialStream, LinesCodec>,
    pulse: Option<Arc<Mutex<PulseController>>>,
) -> Result<()> {
    // Initial handshake
    let handshake = IncomingMessage::Handshake {
        message: "Tek'ma'te Teal'c".parse().unwrap(),
    };
    let j = serde_json::to_string(&handshake)?;
    framed.send(j).await?;

    loop {
        match tokio::time::timeout(Duration::from_secs(2), framed.next()).await {
            Ok(Some(Ok(line))) => {
                let msg: OutgoingMessage = serde_json::from_str(&line)?;
                match msg {
                    OutgoingMessage::Handshake { message } => {
                        if message != "Tek'ma'te Bra'tac" {
                            return Err(anyhow::anyhow!("Handshake mismatch: {}", message));
                        }
                        println!("{}", "Handshake successful!".green());
                        break;
                    }
                    OutgoingMessage::Log { .. } | OutgoingMessage::Volume { .. } => {
                        handle_message(msg, pulse.as_ref());
                    }
                    OutgoingMessage::Pong { .. } => {
                        // Ignore pongs during handshake phase
                    }
                }
            }
            Ok(None) | Ok(Some(Err(_))) | Err(_) => {
                return Err(anyhow::anyhow!("Handshake timeout or error"));
            }
        }
    }

    let mut ping_interval = interval(Duration::from_secs(1));
    let mut ping_timestamp: u64 = 0;
    let mut missed_pings = 0;

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if missed_pings >= 3 {
                    return Err(anyhow::anyhow!("Connection dead: 3 pings missed"));
                }

                ping_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis() as u64;
                let ping = IncomingMessage::Ping {
                    timestamp: ping_timestamp,
                };
                let j = serde_json::to_string(&ping)?;
                framed.send(j).await?;
                missed_pings += 1;
            }
            line = framed.next() => {
                let line = match line {
                    Some(Ok(l)) => l,
                    Some(Err(e)) => return Err(e.into()),
                    None => return Err(anyhow::anyhow!("EOF reached")),
                };

                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                match serde_json::from_str::<OutgoingMessage>(line) {
                    Ok(msg) => {
                        if let OutgoingMessage::Pong { timestamp } = msg {
                            if timestamp == ping_timestamp {
                                missed_pings = 0;
                            }
                        } else {
                            handle_message(msg, pulse.as_ref());
                        }
                    }
                    Err(e) => {
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
        }
    }
}

/// Dispatches an incoming `OutgoingMessage` to the appropriate display logic.
fn handle_message(
    msg: OutgoingMessage,
    pulse: Option<&Arc<Mutex<PulseController>>>,
) {
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

            // Update system volume
            if let Some(p) = pulse {
                if let Ok(mut p_guard) = p.lock() {
                    p_guard.set_volume(value);
                }
            }
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
        OutgoingMessage::Pong { .. } => {}
        OutgoingMessage::Handshake { message } => {
            println!("{} [{}] Unexpected handshake response: {}", now, "HEALTH".yellow().bold(), message);
        }
    }
}
