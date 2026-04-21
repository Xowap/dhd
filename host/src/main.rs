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
use common::{IncomingMessage, OutgoingMessage, SystemMode};
use fs2::FileExt;
use futures::{SinkExt, StreamExt};
use ksni::TrayMethods;
use libpulse_binding as pulse;
use pulse::context::subscribe::Facility;
use pulse::context::{Context, State};
use pulse::mainloop::threaded::Mainloop;
use pulse::volume::{ChannelVolumes, Volume};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::interval;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tokio_util::codec::{Framed, LinesCodec};

/// Raspberry Pi Pico Vendor ID.
const VID: u16 = 0x2e8a;
/// Raspberry Pi Pico CDC-ACM Product ID.
const PID: u16 = 0x000a;

use clap::{Parser, Subcommand, ValueEnum};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

/// DHD (Dial Hifi Device) Host Tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Set the log level for the device
    #[arg(short, long, value_enum, default_value_t = LogLevel::Info, env = "DHD_LOG")]
    log_level: LogLevel,

    /// Enable system tray icon and detach from terminal
    #[arg(short, long)]
    systray: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the DHD host (default)
    Run,
    /// Install the DHD host to autostart and system menu
    Install,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, PartialOrd)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn from_str(s: &str) -> Self {
        match s {
            "ERROR" => LogLevel::Error,
            "WARN" => LogLevel::Warn,
            "INFO" => LogLevel::Info,
            "DEBUG" => LogLevel::Debug,
            "TRACE" => LogLevel::Trace,
            _ => LogLevel::Info,
        }
    }
}

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
struct PulseController {
    mainloop: Mainloop,
    context: Context,
    cache: Arc<Mutex<PulseCache>>,
    vol_tx: mpsc::UnboundedSender<f32>,
}

struct DhdTray {
    connected: bool,
    mode: SystemMode,
    sig_tx: mpsc::UnboundedSender<()>,
}

impl ksni::Tray for DhdTray {
    fn id(&self) -> String {
        "dhd-host".into()
    }

    fn icon_name(&self) -> String {
        let name = if self.connected {
            match self.mode {
                SystemMode::Init => "dhd-init",
                SystemMode::Calibration => "dhd-calib",
                SystemMode::Standby => "dhd-ready",
                SystemMode::Failsafe => "dhd-error",
                SystemMode::PhysicallyDriven => "dhd-hand",
                SystemMode::LogicallyDriven => "dhd-auto",
            }
        } else {
            "dhd-offline"
        };

        let home = std::env::var("HOME").unwrap_or_default();
        let installed_path = PathBuf::from(home)
            .join(".local/share/dhd/assets")
            .join(format!("{}.svg", name));

        installed_path.to_string_lossy().into_owned()
    }

    fn title(&self) -> String {
        let emoji = if self.connected {
            match self.mode {
                SystemMode::Init => "⏳",
                SystemMode::Calibration => "⚖️",
                SystemMode::Standby => "😴",
                SystemMode::Failsafe => "🚨",
                SystemMode::PhysicallyDriven => "🖐️",
                SystemMode::LogicallyDriven => "🤖",
            }
        } else {
            "❌"
        };
        format!("{} DHD", emoji)
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let status = if self.connected {
            match self.mode {
                SystemMode::Init => "Initializing...",
                SystemMode::Calibration => "Calibrating...",
                SystemMode::Standby => "Standby",
                SystemMode::Failsafe => "FAILSAFE!",
                SystemMode::PhysicallyDriven => "Physically Driven",
                SystemMode::LogicallyDriven => "Logically Driven",
            }
        } else {
            "Disconnected"
        };
        ksni::ToolTip {
            title: "DHD Host".into(),
            description: status.into(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Force Calibration".into(),
                enabled: self.connected,
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sig_tx.send(());
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_| {
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

unsafe impl Send for PulseController {}
unsafe impl Sync for PulseController {}

fn log(_tag: &str, color: ColoredString, message: impl AsRef<str>) {
    let now = Local::now().format("%H:%M:%S%.3f").to_string().dimmed();
    println!("{} [{}] {}", now, color.bold(), message.as_ref());
}

impl PulseController {
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

    fn get_volume(&self) -> f32 {
        let last_vol = {
            let c = self.cache.lock().unwrap();
            c.last_volume
        };
        last_vol.avg().0 as f32 / Volume::NORMAL.0 as f32
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Commands::Install) = cli.command {
        return install();
    }
    let home = std::env::var("HOME").context("Failed to get HOME")?;
    let lock_path = PathBuf::from(home).join(".cache/dhd.lock");
    std::fs::create_dir_all(lock_path.parent().unwrap())?;
    {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        if f.try_lock_exclusive().is_err() {
            eprintln!(
                "\n{} {}\n",
                "⚠️".yellow(),
                "Another instance of DHD is already running. Aborting."
                    .bold()
                    .red()
            );
            std::process::exit(1);
        }
    }
    if cli.systray {
        let stdout = std::fs::File::create("/tmp/dhd-host.out")?;
        let stderr = std::fs::File::create("/tmp/dhd-host.err")?;
        let daemonize = daemonize::Daemonize::new()
            .stdout(stdout)
            .stderr(stderr)
            .working_directory("/");
        if let Err(e) = daemonize.start() {
            eprintln!("{} Error daemonizing: {}", "⚠️".red(), e);
            std::process::exit(1);
        }
    }
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file
        .lock_exclusive()
        .context("Failed to acquire process lock")?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let _holder = lock_file;
            run(cli).await
        })
}

fn install() -> Result<()> {
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let home = std::env::var("HOME").context("Failed to get HOME environment variable")?;
    let bin_dir = PathBuf::from(&home).join(".local/bin");
    let target_exe = bin_dir.join("dhd");
    let data_dir = PathBuf::from(&home).join(".local/share/dhd/assets");
    let autostart_dir = PathBuf::from(&home).join(".config/autostart");
    let apps_dir = PathBuf::from(&home).join(".local/share/applications");
    let icons_dir = PathBuf::from(&home).join(".local/share/icons/hicolor/scalable/apps");
    std::fs::create_dir_all(&bin_dir)?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&autostart_dir)?;
    std::fs::create_dir_all(&apps_dir)?;
    std::fs::create_dir_all(&icons_dir)?;
    
    if current_exe.canonicalize().unwrap_or_else(|_| current_exe.clone()) != target_exe.canonicalize().unwrap_or_else(|_| target_exe.clone()) {
        let _ = std::fs::remove_file(&target_exe);
        std::fs::copy(&current_exe, &target_exe)?;
    }

    const ASSETS: &[(&str, &[u8])] = &[
        ("dhd-auto.svg", include_bytes!("../assets/dhd-auto.svg")),
        ("dhd-calib.svg", include_bytes!("../assets/dhd-calib.svg")),
        ("dhd-error.svg", include_bytes!("../assets/dhd-error.svg")),
        ("dhd-hand.svg", include_bytes!("../assets/dhd-hand.svg")),
        ("dhd-init.svg", include_bytes!("../assets/dhd-init.svg")),
        (
            "dhd-offline.svg",
            include_bytes!("../assets/dhd-offline.svg"),
        ),
        ("dhd-ready.svg", include_bytes!("../assets/dhd-ready.svg")),
    ];

    let mut asset_count = 0;
    for (name, content) in ASSETS {
        let dest = data_dir.join(name);
        std::fs::write(&dest, content)?;
        asset_count += 1;

        if *name == "dhd-ready.svg" {
            let icon_file = icons_dir.join("dhd.svg");
            std::fs::write(&icon_file, content)?;
        }
    }

    let desktop_content = format!(
        r#"[Desktop Entry]
Type=Application
Name=DHD Host
Exec={} --systray
Icon=dhd
Comment=Dial Hifi Device Host
Terminal=false
Categories=Settings;HardwareSettings;
X-GNOME-Autostart-enabled=true
"#,
        target_exe.display()
    );
    std::fs::write(autostart_dir.join("dhd.desktop"), &desktop_content)?;
    std::fs::write(apps_dir.join("dhd.desktop"), &desktop_content)?;
    let pipe = "│".truecolor(100, 100, 100);
    let branch = "├──".truecolor(100, 100, 100);
    let last = "└──".truecolor(100, 100, 100);
    println!("\n{}", "🚀 DHD Installation Complete!".bold().cyan());
    println!("{}", "~/.local/".blue().bold());
    println!("{} {}", branch, "bin/".blue().bold());
    println!("{}   {} {}", pipe, last, "dhd".green().bold());
    println!("{} {}", branch, "share/".blue().bold());
    println!("{}   {} {}", pipe, branch, "dhd/".blue().bold());
    println!(
        "{}   {}   {} {} {}",
        pipe,
        pipe,
        last,
        "assets/".blue().bold(),
        format!("({} files)", asset_count).yellow()
    );
    println!("{}   {} {}", pipe, branch, "applications/".blue().bold());
    println!("{}   {}   {} {}", pipe, pipe, last, "dhd.desktop".green());
    println!(
        "{}   {} {}",
        pipe,
        last,
        "icons/hicolor/scalable/apps/".blue().bold()
    );
    println!("{}       {} {}", pipe, last, "dhd.svg".green());
    println!("{}", ".config/autostart/".blue().bold());
    println!("{} {}", last, "dhd.desktop".green());
    println!("\n{}\n", "✨ Ready to roll!".bold().green());
    Ok(())
}

async fn run(args: Cli) -> Result<()> {
    println!("{}", "=== DHD Host Starting ===".bold().cyan());
    let (sig_tx, sig_rx) = mpsc::unbounded_channel();
    let tray_handle = if args.systray {
        let tray = DhdTray {
            connected: false,
            mode: SystemMode::Init,
            sig_tx: sig_tx.clone(),
        };
        Some(tray.spawn().await.expect("Failed to spawn tray"))
    } else {
        None
    };
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
    let mut sig_usr1 = signal(SignalKind::user_defined1())?;
    let sig_tx_clone = sig_tx.clone();
    tokio::spawn(async move {
        while sig_usr1.recv().await.is_some() {
            log(
                "HOST",
                "HOST".yellow(),
                "Received SIGUSR1 - Triggering hard calibration",
            );
            let _ = sig_tx_clone.send(());
        }
    });
    let mut sig_rx_opt = Some(sig_rx);
    loop {
        match find_and_connect() {
            Ok(stream) => {
                log("DEVICE", "DEVICE".green(), "Connected to DHD device!");
                if let Some(handle) = tray_handle.as_ref() {
                    let _ = handle
                        .update(|tray| {
                            tray.connected = true;
                        })
                        .await;
                }
                let framed = Framed::new(stream, LinesCodec::new());
                if let Err(e) = run_host(
                    framed,
                    pulse.clone(),
                    &mut vol_rx,
                    &mut sig_rx_opt,
                    &args,
                    &tray_handle,
                )
                .await
                {
                    log("DEVICE", "DEVICE".red(), format!("Connection lost: {}", e));
                }
                if let Some(handle) = tray_handle.as_ref() {
                    let _ = handle
                        .update(|tray| {
                            tray.connected = false;
                        })
                        .await;
                }
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

fn find_and_connect() -> Result<SerialStream> {
    let ports = serialport::available_ports().context("Failed to list serial ports")?;
    for p in ports {
        if let serialport::SerialPortType::UsbPort(info) = p.port_type
            && info.vid == VID
            && info.pid == PID
        {
            return tokio_serial::new(p.port_name, 115_200)
                .open_native_async()
                .context("Failed to open serial port");
        }
    }
    anyhow::bail!("Device not found")
}

async fn run_host(
    mut framed: Framed<SerialStream, LinesCodec>,
    pulse: Option<Arc<Mutex<PulseController>>>,
    vol_rx: &mut Option<mpsc::UnboundedReceiver<f32>>,
    sig_rx: &mut Option<mpsc::UnboundedReceiver<()>>,
    args: &Cli,
    tray_handle: &Option<ksni::Handle<DhdTray>>,
) -> Result<()> {
    let handshake = IncomingMessage::Handshake {
        message: "Tek'ma'te Teal'c".parse().unwrap(),
    };
    framed.send(serde_json::to_string(&handshake)?).await?;
    if let Some(p) = pulse.clone() {
        let vol = p.lock().ok().map(|p_guard| p_guard.get_volume());
        if let Some(v) = vol {
            let set_vol = IncomingMessage::StartCalibration {
                volume: v,
                force: false,
            };
            framed.send(serde_json::to_string(&set_vol)?).await?;
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
                    _ => handle_message(msg, pulse.as_ref(), args, tray_handle).await,
                }
            }
            _ => return Err(anyhow::anyhow!("Handshake timeout or error")),
        }
    }
    let mut ping_interval = interval(Duration::from_secs(1));
    let mut ping_timestamp: u64 = 0;
    let mut missed_pings = 0;
    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if missed_pings >= 3 { return Err(anyhow::anyhow!("Connection dead")); }
                ping_timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64;
                framed.send(serde_json::to_string(&IncomingMessage::Ping { timestamp: ping_timestamp })?).await?;
                missed_pings += 1;
            }
            Some(vol) = async { if let Some(rx) = vol_rx.as_mut() { rx.recv().await } else { futures::future::pending().await } } => {
                framed.send(serde_json::to_string(&IncomingMessage::SetVolume { value: vol })?).await?;
            }
            Some(_) = async { if let Some(rx) = sig_rx.as_mut() { rx.recv().await } else { futures::future::pending().await } } => {
                let vol = pulse.as_ref().and_then(|p| p.lock().ok()).map(|p| p.get_volume()).unwrap_or(0.5);
                framed.send(serde_json::to_string(&IncomingMessage::StartCalibration { volume: vol, force: true })?).await?;
            }
            line = framed.next() => {
                let line = match line { Some(Ok(l)) => l, Some(Err(e)) => return Err(e.into()), None => return Err(anyhow::anyhow!("EOF")), };
                if let Ok(msg) = serde_json::from_str::<OutgoingMessage>(line.trim()) {
                    if let OutgoingMessage::Pong { timestamp } = msg { if timestamp == ping_timestamp { missed_pings = 0; } }
                    else { handle_message(msg, pulse.as_ref(), args, tray_handle).await; }
                }
            }
        }
    }
}

async fn handle_message(
    msg: OutgoingMessage,
    pulse: Option<&Arc<Mutex<PulseController>>>,
    args: &Cli,
    tray_handle: &Option<ksni::Handle<DhdTray>>,
) {
    match msg {
        OutgoingMessage::Volume { value } => {
            let bar_len = (value * 20.0).clamp(0.0, 20.0) as usize;
            log(
                "VOL",
                "VOL".blue(),
                format!(
                    "{} {:.3}",
                    "|".repeat(bar_len) + &"-".repeat(20 - bar_len),
                    value
                ),
            );
            if let Some(p) = pulse
                && let Ok(mut p_guard) = p.lock()
            {
                p_guard.set_volume(value);
            }
        }
        OutgoingMessage::Log { level, message } => {
            if LogLevel::from_str(level.as_str()) <= args.log_level {
                let lvl_colored = match level.as_str() {
                    "INFO" => "INFO".green(),
                    "WARN" => "WARN".yellow(),
                    "ERROR" => "ERROR".red(),
                    "DEBUG" => "DEBUG".blue(),
                    "TRACE" => "TRACE".magenta(),
                    _ => level.as_str().normal(),
                };
                log(
                    "LOG",
                    "LOG".white(),
                    format!("[{}] {}", lvl_colored, message),
                );
            }
        }
        OutgoingMessage::Mode { mode: new_mode } => {
            log("MODE", "MODE".magenta(), format!("{:?}", new_mode));
            if let Some(handle) = tray_handle {
                let _ = handle
                    .update(|tray| {
                        tray.mode = new_mode;
                    })
                    .await;
            }
        }
        _ => {}
    }
}
