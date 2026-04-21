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
use pulse::context::subscribe::Facility;
use pulse::context::{Context, State};
use pulse::mainloop::threaded::Mainloop;
use pulse::volume::{ChannelVolumes, Volume};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::interval;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tokio_util::codec::{Framed, LinesCodec};

/// Raspberry Pi Pico Vendor ID.
const VID: u16 = 0x2e8a;
/// Raspberry Pi Pico CDC-ACM Product ID.
const PID: u16 = 0x000a;

use tokio::sync::mpsc;

/// PulseAudio events we want to handle.
enum PulseEvent {
    ServerChange,
    SinkChange(u32),
}

#[derive(Default)]
struct PulseCache {
    sink_index: Option<u32>,
    num_channels: u8,
    last_volume: ChannelVolumes,
}

/// PulseAudio controller for system volume adjustment.
///
/// Uses a threaded mainloop to handle PulseAudio events and callbacks
/// asynchronously from the main Tokio loop.
struct PulseController {
    mainloop: Mainloop,
    context: Context,
    cache: Arc<Mutex<PulseCache>>,
    vol_tx: mpsc::UnboundedSender<f32>,
}

// PulseController is safe to share across threads because we use the
// ThreadedMainloop's locking mechanism to synchronize access to the context.
unsafe impl Send for PulseController {}
unsafe impl Sync for PulseController {}

/// Formats and prints a log message with a timestamp.
fn log(_tag: &str, color: ColoredString, message: impl AsRef<str>) {
    let now = Local::now().format("%H:%M:%S%.3f").to_string().dimmed();
    println!("{} [{}] {}", now, color.bold(), message.as_ref());
}

impl PulseController {
    /// Creates a new `PulseController` instance, establishing a connection to 
    /// the PulseAudio server.
    ///
    /// It spins up a threaded mainloop and subscribes to server and sink events 
    /// to track changes to the default audio output device. It also returns two 
    /// receiver channels: one for raw PulseAudio events, and one specifically 
    /// for external volume changes.
    fn new() -> Result<(
        Self,
        mpsc::UnboundedReceiver<PulseEvent>,
        mpsc::UnboundedReceiver<f32>,
    )> {
        let mut mainloop = Mainloop::new()
            .ok_or_else(|| anyhow::anyhow!("Failed to create PulseAudio mainloop"))?;

        let mut context = Context::new(&mainloop, "DHD Host")
            .ok_or_else(|| anyhow::anyhow!("Failed to create PulseAudio context"))?;

        context
            .connect(None, pulse::context::FlagSet::NOFLAGS, None)
            .map_err(|e| anyhow::anyhow!("Failed to connect PulseAudio context: {:?}", e))?;

        mainloop
            .start()
            .map_err(|e| anyhow::anyhow!("Failed to start PulseAudio mainloop: {:?}", e))?;

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

        let cache = Arc::new(Mutex::new(PulseCache::default()));
        let (tx, rx) = mpsc::unbounded_channel();
        let (vol_tx, vol_rx) = mpsc::unbounded_channel();

        mainloop.lock();
        context.set_subscribe_callback(Some(Box::new(move |facility, _op, index| {
            if let Some(Facility::Server) = facility {
                let _ = tx.send(PulseEvent::ServerChange);
            } else if let Some(Facility::Sink) = facility {
                let _ = tx.send(PulseEvent::SinkChange(index));
            }
        })));

        context.subscribe(
            pulse::context::subscribe::InterestMaskSet::SERVER
                | pulse::context::subscribe::InterestMaskSet::SINK,
            |_| {},
        );

        let cache_initial = Arc::clone(&cache);
        context
            .introspect()
            .get_sink_info_by_name("@DEFAULT_SINK@", move |res| {
                if let pulse::callbacks::ListResult::Item(info) = res {
                    Self::update_cache_and_log(&cache_initial, info, "Default sink");
                }
            });

        mainloop.unlock();

        Ok((
            Self {
                mainloop,
                context,
                cache,
                vol_tx,
            },
            rx,
            vol_rx,
        ))
    }

    fn update_cache_and_log(
        cache: &Arc<Mutex<PulseCache>>,
        info: &pulse::context::introspect::SinkInfo,
        label: &str,
    ) {
        let name = info.name.as_deref().unwrap_or("unknown");
        let desc = info.description.as_deref().unwrap_or("no description");
        let current_vol = info.volume.avg().0 as f32 / Volume::NORMAL.0 as f32;

        log(
            "PULSE",
            "PULSE".magenta(),
            format!(
                "{}: {} ({}) [vol: {:.3}]",
                label,
                name.cyan(),
                desc.italic().dimmed(),
                current_vol
            ),
        );

        if let Ok(mut c) = cache.lock() {
            c.sink_index = Some(info.index);
            c.num_channels = info.volume.get().len() as u8;
            c.last_volume = info.volume;
        }
    }

    /// Dispatches a raw PulseAudio event, updating the internal cache if the 
    /// default sink changes or if its volume is modified externally. Emits the 
    /// new volume to the `vol_tx` channel if changed.
    pub fn handle_event(&mut self, event: PulseEvent) {
        self.mainloop.lock();
        match event {
            PulseEvent::ServerChange => {
                let cache_inner = Arc::clone(&self.cache);
                self.context
                    .introspect()
                    .get_sink_info_by_name("@DEFAULT_SINK@", move |res| {
                        if let pulse::callbacks::ListResult::Item(info) = res {
                            Self::update_cache_and_log(&cache_inner, info, "Default sink changed");
                        }
                    });
            }
            PulseEvent::SinkChange(index) => {
                let (target_index, last_vol) = {
                    let c = self.cache.lock().unwrap();
                    (c.sink_index, c.last_volume)
                };

                if Some(index) == target_index {
                    let cache_inner = Arc::clone(&self.cache);
                    let vol_tx = self.vol_tx.clone();
                    self.context
                        .introspect()
                        .get_sink_info_by_index(index, move |res| {
                            if let pulse::callbacks::ListResult::Item(info) = res
                                && info.volume != last_vol
                                && let Ok(mut c) = cache_inner.lock()
                                && info.volume != c.last_volume
                            {
                                let avg_vol = info.volume.avg().0 as f32 / Volume::NORMAL.0 as f32;
                                c.last_volume = info.volume;
                                let _ = vol_tx.send(avg_vol);
                                log(
                                    "PULSE",
                                    "PULSE".green(),
                                    format!("External volume change: {:.3}", avg_vol),
                                );
                            }
                        });
                }
            }
        }
        self.mainloop.unlock();
    }

    /// Updates the system volume for the default sink to the specified 
    /// normalized value (0.0 to 1.0).
    fn set_volume(&mut self, value: f32) {
        let vol = Volume((Volume::NORMAL.0 as f32 * value) as u32);

        self.mainloop.lock();

        let (index, n_channels) = {
            let c = self.cache.lock().unwrap();
            (c.sink_index, c.num_channels)
        };

        if let Some(idx) = index {
            let mut cv = ChannelVolumes::default();
            cv.set(n_channels, vol);

            if let Ok(mut c) = self.cache.lock() {
                c.last_volume = cv;
            }

            self.context
                .introspect()
                .set_sink_volume_by_index(idx, &cv, None);
        }

        self.mainloop.unlock();
    }

    /// Retrieves the current normalized volume (0.0 to 1.0) of the default 
    /// sink from the cache.
    fn get_volume(&self) -> f32 {
        let last_vol = {
            let c = self.cache.lock().unwrap();
            c.last_volume
        };
        last_vol.avg().0 as f32 / Volume::NORMAL.0 as f32
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "=== DHD Host Starting ===".bold().cyan());

    let (pulse, pulse_rx, mut vol_rx) = match PulseController::new() {
        Ok((p, rx, vrx)) => (Some(Arc::new(Mutex::new(p))), Some(rx), Some(vrx)),
        Err(e) => {
            eprintln!(
                "{} {}",
                "Warning: PulseAudio connection failed:".yellow(),
                e
            );
            (None, None, None)
        }
    };

    if let (Some(p), Some(mut rx)) = (pulse.clone(), pulse_rx) {
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Ok(mut p_guard) = p.lock() {
                    p_guard.handle_event(event);
                }
            }
        });
    }

    loop {
        match find_and_connect() {
            Ok(stream) => {
                log("DEVICE", "DEVICE".green(), "Connected to DHD device!");
                let framed = Framed::new(stream, LinesCodec::new());
                if let Err(e) = run_host(framed, pulse.clone(), &mut vol_rx).await {
                    log("DEVICE", "DEVICE".red(), format!("Connection lost: {}", e));
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
        if let serialport::SerialPortType::UsbPort(info) = p.port_type
            && info.vid == VID
            && info.pid == PID
        {
            let stream = tokio_serial::new(p.port_name, 115_200)
                .open_native_async()
                .context("Failed to open serial port")?;
            return Ok(stream);
        }
    }
    anyhow::bail!("Device not found")
}

/// Main communication loop for an active connection.
async fn run_host(
    mut framed: Framed<SerialStream, LinesCodec>,
    pulse: Option<Arc<Mutex<PulseController>>>,
    vol_rx: &mut Option<mpsc::UnboundedReceiver<f32>>,
) -> Result<()> {
    // Initial handshake
    let handshake = IncomingMessage::Handshake {
        message: "Tek'ma'te Teal'c".parse().unwrap(),
    };
    let j = serde_json::to_string(&handshake)?;
    framed.send(j).await?;

    // Send initial volume to the device immediately after handshake
    if let Some(p) = pulse.clone() {
        let vol = if let Ok(p_guard) = p.lock() {
            Some(p_guard.get_volume())
        } else {
            None
        };

        if let Some(v) = vol {
            let set_vol = IncomingMessage::StartCalibration { volume: v };
            let j = serde_json::to_string(&set_vol)?;
            framed.send(j).await?;
        }
    }

    loop {
        match tokio::time::timeout(Duration::from_secs(2), framed.next()).await {
            Ok(Some(Ok(line))) => {
                let msg: OutgoingMessage = serde_json::from_str(&line)?;
                match msg {
                    OutgoingMessage::Handshake { message } => {
                        if message != "Tek'ma'te Bra'tac" {
                            return Err(anyhow::anyhow!("Handshake mismatch: {}", message));
                        }
                        log("DEVICE", "DEVICE".green(), "Handshake successful!");
                        break;
                    }
                    OutgoingMessage::Log { .. }
                    | OutgoingMessage::Volume { .. }
                    | OutgoingMessage::Mode { .. } => {
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
            Some(vol) = async {
                if let Some(rx) = vol_rx.as_mut() {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            } => {
                let set_vol = IncomingMessage::SetVolume { value: vol };
                if let Ok(j) = serde_json::to_string(&set_vol) {
                    let _ = framed.send(j).await;
                }
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
fn handle_message(msg: OutgoingMessage, pulse: Option<&Arc<Mutex<PulseController>>>) {
    match msg {
        OutgoingMessage::Volume { value } => {
            let bar_len = (value * 20.0).clamp(0.0, 20.0) as usize;
            let bar = "|".repeat(bar_len) + &"-".repeat(20 - bar_len);
            log("VOL", "VOL".blue(), format!("{} {:.3}", bar.blue(), value));

            // Update system volume
            if let Some(p) = pulse
                && let Ok(mut p_guard) = p.lock()
            {
                p_guard.set_volume(value);
            }
        }
        OutgoingMessage::Log { level, message } => {
            let lvl = match level.as_str() {
                "INFO" => "INFO".green(),
                "WARN" => "WARN".yellow(),
                "ERROR" => "ERROR".red(),
                _ => level.as_str().normal(),
            };
            log("LOG", "LOG".white(), format!("[{}] {}", lvl, message));
        }
        OutgoingMessage::Mode { mode } => {
            log("MODE", "MODE".magenta(), format!("{:?}", mode));
        }
        OutgoingMessage::Pong { .. } => {}
        OutgoingMessage::Handshake { message } => {
            log(
                "HEALTH",
                "HEALTH".yellow(),
                format!("Unexpected handshake response: {}", message),
            );
        }
    }
}
