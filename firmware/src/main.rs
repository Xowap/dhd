//! DHD (Dial Hifi Device) Firmware
//!
//! A physical media controller for computers, providing tactile control over volume
//! and media playback with future support for haptic feedback.
//!
//! This firmware implements a dependency-injection-based architecture. Hardware sensors
//! (like the potentiometer) are sampled in dedicated tasks and publish their state to
//! shared atomic storage. Other tasks, such as the system reporter or HID reports,
//! consume this data to interact with the host computer over JSON-over-CDC-ACM.

#![no_std]
#![no_main]

use common::{IncomingMessage, OutgoingMessage};
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_executor::Spawner;
use embassy_rp::adc::{
    Adc, Async, Channel, Config as AdcConfig, InterruptHandler as AdcInterruptHandler,
};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler as UsbInterruptHandler};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel as MsgChannel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Ticker, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, Config};
use heapless::String;
use static_cell::StaticCell;
use {panic_halt as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => UsbInterruptHandler<USB>;
    ADC_IRQ_FIFO => AdcInterruptHandler;
});

/// Program metadata for `picotool info`.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"DHD - Dial Hifi Device"),
    embassy_rp::binary_info::rp_program_description!(
        c"Physical media controller with haptic volume feedback"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

/// The `VolumeState` acts as the single source of truth for the device's current volume level.
///
/// It facilitates communication between the sensor manager (which samples the physical dial)
/// and the system observers (reporter, future HID reports). By using atomic operations, it allows
/// these tasks to interact safely without the overhead of a mutex, keeping the data
/// flow responsive and thread-safe across the async executor.
struct VolumeState {
    /// Volume stored as parts per million (0 to 1,000,000).
    /// Atomic storage ensures we can update and read the intensity across different
    /// execution contexts without data races.
    raw_ppm: AtomicU32,
}

impl VolumeState {
    /// Creates a new `VolumeState` instance initialized to zero.
    const fn new() -> Self {
        Self {
            raw_ppm: AtomicU32::new(0),
        }
    }

    /// Sets the current volume level.
    ///
    /// This is called by sensor drivers like `PotentiometerManager` to publish
    /// new data read from the physical dial.
    fn set(&self, val: f32) {
        let val = (val.clamp(0.0, 1.0) * 1_000_000.0) as u32;
        self.raw_ppm.store(val, Ordering::Relaxed);
    }

    /// Returns the current volume level.
    ///
    /// This is used by observers like the `Reporter` task to retrieve the latest
    /// volume state for diagnostics or computer communication.
    fn get(&self) -> f32 {
        self.raw_ppm.load(Ordering::Relaxed) as f32 / 1_000_000.0
    }
}

/// A channel to multiplex messages from various tasks to the USB CDC-ACM task.
static OUTGOING_CHANNEL: MsgChannel<CriticalSectionRawMutex, OutgoingMessage, 8> = MsgChannel::new();

/// `JsonLogger` provides a custom `log` backend that redirects logs to the host.
///
/// Instead of printing to a standard console, it serializes logs into `OutgoingMessage::Log`
/// objects and puts them into the `OUTGOING_CHANNEL` for delivery over USB.
struct JsonLogger;

impl log::Log for JsonLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let mut level_str = String::<16>::new();
            let _ = core::fmt::write(&mut level_str, format_args!("{}", record.level()));

            let mut msg_str = String::<128>::new();
            let _ = core::fmt::write(&mut msg_str, format_args!("{}", record.args()));

            let _ = OUTGOING_CHANNEL.try_send(OutgoingMessage::Log {
                level: level_str,
                message: msg_str,
            });
        }
    }

    fn flush(&self) {}
}

static LOGGER: JsonLogger = JsonLogger;

/// `PotentiometerManager` handles reading the analog value from the physical DHD dial.
///
/// It acts as a hardware sensor driver that periodically samples the ADC and translates
/// the raw voltage into a normalized float. By updating the `VolumeState`, it
/// provides a bridge between physical user interaction and the rest of the DHD's
/// logic, enabling the device to track knob movement in real-time.
struct PotentiometerManager<const N: usize> {
    adc: Adc<'static, Async>,
    channel: Channel<'static>,
    state: &'static VolumeState,
    interval: Duration,
    bottom: f32,
    top: f32,
    buffer: [u16; N],
    index: usize,
    count: usize,
    hysteresis_band: u16,
    last_stable_val: Option<u16>,
    on_change: &'static Signal<CriticalSectionRawMutex, ()>,
}

impl<const N: usize> PotentiometerManager<N> {
    /// Creates a new `PotentiometerManager` with the specified calibration and timing.
    ///
    /// * `adc`: The initialized ADC peripheral.
    /// * `channel`: The specific ADC channel (GP26) connected to the dial wiper.
    /// * `state`: The `VolumeState` to update with new readings.
    /// * `interval`: How often to sample the hardware.
    /// * `bottom`: Clipping level for the low end (clamped to 0.0 below this).
    /// * `top`: Clipping level for the high end (clamped to 1.0 above this).
    /// * `hysteresis_band`: The hysteresis band in LSB.
    /// * `on_change`: A signal used to notify other tasks when the value changes.
    fn new(
        adc: Adc<'static, Async>,
        channel: Channel<'static>,
        state: &'static VolumeState,
        interval: Duration,
        bottom: f32,
        top: f32,
        hysteresis_band: u16,
        on_change: &'static Signal<CriticalSectionRawMutex, ()>,
    ) -> Self {
        Self {
            adc,
            channel,
            state,
            interval,
            bottom,
            top,
            buffer: [0; N],
            index: 0,
            count: 0,
            hysteresis_band,
            last_stable_val: None,
            on_change,
        }
    }

    /// Continuously polls the ADC and updates the shared volume state.
    ///
    /// This loop converts the raw 12-bit ADC range into a normalized 0.0..1.0 range,
    /// applying a median filter and hysteresis to ensure smooth updates.
    async fn run(mut self) {
        let mut ticker = Ticker::every(self.interval);
        loop {
            if let Ok(raw_val) = self.adc.read(&mut self.channel).await {
                self.buffer[self.index] = raw_val;
                self.index = (self.index + 1) % N;
                if self.count < N {
                    self.count += 1;
                }

                let mut sort_buf = [0u16; N];
                sort_buf[..self.count].copy_from_slice(&self.buffer[..self.count]);
                sort_buf[..self.count].sort_unstable();
                let median_val = sort_buf[self.count / 2];

                let mut changed = false;
                let stable_val = if let Some(last) = self.last_stable_val {
                    if (median_val as i32 - last as i32).abs() > self.hysteresis_band as i32 {
                        self.last_stable_val = Some(median_val);
                        changed = true;
                        median_val
                    } else {
                        last
                    }
                } else {
                    self.last_stable_val = Some(median_val);
                    changed = true;
                    median_val
                };

                let val = stable_val as f32 / 4095.0;
                let normalized = if val <= self.bottom {
                    0.0
                } else if val >= self.top {
                    1.0
                } else {
                    (val - self.bottom) / (self.top - self.bottom)
                };

                if changed {
                    self.state.set(normalized);
                    self.on_change.signal(());
                }
            }
            ticker.next().await;
        }
    }
}

/// `Reporter` provides system observability and diagnostics for the DHD.
///
/// It listens for changes in the potentiometer and pushes the new volume state
/// to the outgoing USB channel for host monitoring.
struct Reporter {
    state: &'static VolumeState,
    on_change: &'static Signal<CriticalSectionRawMutex, ()>,
}

impl Reporter {
    /// Injects the shared state and change signal that this reporter will observe.
    fn new(state: &'static VolumeState, on_change: &'static Signal<CriticalSectionRawMutex, ()>) -> Self {
        Self { state, on_change }
    }

    /// Sends volume updates over USB whenever a change occurs.
    async fn run(self) {
        loop {
            self.on_change.wait().await;
            let current = self.state.get();
            let _ = OUTGOING_CHANNEL.send(OutgoingMessage::Volume { value: current }).await;
        }
    }
}

/// Embassy task wrapper for the Potentiometer polling loop.
#[embassy_executor::task]
async fn potentiometer_task(manager: PotentiometerManager<15>) {
    manager.run().await;
}

/// Embassy task wrapper for the system reporter.
#[embassy_executor::task]
async fn reporter_task(reporter: Reporter) {
    reporter.run().await;
}

/// `usb_task` manages the lifecycle of the USB device and its communication classes.
///
/// It handles device enumeration and manages the serial port (CDC-ACM) communication
/// by running the `run_serial` loop when a host connection is active.
#[embassy_executor::task]
async fn usb_task(builder: Builder<'static, Driver<'static, USB>>, mut class: CdcAcmClass<'static, Driver<'static, USB>>) {
    let mut usb = builder.build();
    let usb_fut = usb.run();

    let echo_fut = async {
        loop {
            class.wait_connection().await;
            let _ = run_serial(&mut class).await;
        }
    };

    embassy_futures::select::select(usb_fut, echo_fut).await;
}

/// `run_serial` implements the actual bi-directional JSON communication protocol over USB.
///
/// It performs two main roles:
/// 1. Processes incoming packets from the host (e.g., Pings).
/// 2. Drains the `OUTGOING_CHANNEL` and sends JSON messages to the host (e.g., Volume updates, Logs).
async fn run_serial(class: &mut CdcAcmClass<'static, Driver<'static, USB>>) -> Result<(), EndpointError> {
    let mut buf = [0u8; 256];
    loop {
        let read_fut = class.read_packet(&mut buf);
        let send_fut = OUTGOING_CHANNEL.receive();

        match embassy_futures::select::select(read_fut, send_fut).await {
            embassy_futures::select::Either::First(read_res) => {
                let size = read_res?;
                let data = &buf[..size];
                if let Ok((IncomingMessage::Ping { timestamp }, _)) = serde_json_core::from_slice::<IncomingMessage>(data) {
                    let _ = OUTGOING_CHANNEL.try_send(OutgoingMessage::Pong { timestamp });
                }
            }
            embassy_futures::select::Either::Second(msg) => {
                let mut out_buf = [0u8; 256];
                if let Ok(size) = serde_json_core::to_slice(&msg, &mut out_buf) {
                    class.write_packet(&out_buf[..size]).await?;
                    // Add newline for easier parsing on host side
                    class.write_packet(b"\n").await?;
                }
            }
        }
    }
}

/// The main entry point responsible for DHD system orchestration and dependency injection.
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // Initialize custom logger
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);

    // USB Driver Setup: We use CDC-ACM to provide a virtual serial port for host communication.
    let driver = Driver::new(p.USB, Irqs);
    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static STATE: StaticCell<State> = StaticCell::new();

    let mut config = Config::new(0x2e8a, 0x000a); 
    config.manufacturer = Some("Offworld Nexus");
    config.product = Some("Dial Hifi Device");
    config.serial_number = Some("DHD-DEV");
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [], // no msos
        CONTROL_BUF.init([0; 64]),
    );

    let class = CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);

    spawner.spawn(usb_task(builder, class).unwrap());

    // Volume state is allocated in a StaticCell to provide a 'static reference that can
    // be shared safely between multiple tasks.
    static VOLUME_STATE: StaticCell<VolumeState> = StaticCell::new();
    let volume = VOLUME_STATE.init(VolumeState::new());

    // Initialize status LED: Always on to indicate device power.
    let mut led = Output::new(p.PIN_25, Level::High);
    led.set_high();

    // ADC setup: GP26 (Pin 31) is configured for analog input from the dial.
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());
    let channel = Channel::new_pin(p.PIN_26, embassy_rp::gpio::Pull::None);

    // Create a signal to notify the reporter when the potentiometer value changes.
    static POT_CHANGE_SIGNAL: StaticCell<Signal<CriticalSectionRawMutex, ()>> = StaticCell::new();
    let on_change = POT_CHANGE_SIGNAL.init(Signal::new());

    // Dependency Injection: Component assembly.
    let pot_manager = PotentiometerManager::<15>::new(
        adc,
        channel,
        volume,
        Duration::from_millis(2),
        0.02,
        0.98,
        21,
        on_change,
    );
    let reporter = Reporter::new(volume, on_change);

    // Final hand-off to the async executor.
    spawner.spawn(potentiometer_task(pot_manager).unwrap());
    spawner.spawn(reporter_task(reporter).unwrap());

    log::info!("DHD Firmware Initialized");

    // The main task yields control to the other tasks indefinitely.
    loop {
        Timer::after_secs(3600).await;
    }
}
