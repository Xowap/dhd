//! DHD (Dial Hifi Device)
//!
//! A physical media controller for computers, providing tactile control over volume
//! and media playback with future support for haptic feedback.
//!
//! This firmware implements a dependency-injection-based architecture. Hardware sensors
//! (like the potentiometer) are sampled in dedicated tasks and publish their state to
//! shared atomic storage. Other tasks, such as the system monitor or future HID reports,
//! consume this data to interact with the host computer.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};
use log::info;
use embassy_executor::Spawner;
use embassy_rp::adc::{Adc, Channel, Config as AdcConfig, InterruptHandler as AdcInterruptHandler, Async};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler as UsbInterruptHandler};
use embassy_time::{Duration, Ticker, Timer};
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
/// and the system observers (monitor, HID reports). By using atomic operations, it allows
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
    /// This is used by observers like the `Monitor` task to retrieve the latest
    /// volume state for diagnostics or computer communication.
    fn get(&self) -> f32 {
        self.raw_ppm.load(Ordering::Relaxed) as f32 / 1_000_000.0
    }
}

/// `UsbLogger` encapsulates the USB peripheral and the logging runtime.
///
/// By wrapping the driver in a struct, we ensure that the USB hardware is properly
/// owned and that the logging setup is isolated from the rest of the application logic.
struct UsbLogger {
    driver: Driver<'static, USB>,
}

impl UsbLogger {
    /// Creates a logger instance around an initialized USB driver.
    fn new(driver: Driver<'static, USB>) -> Self {
        Self { driver }
    }

    /// Starts the embassy-usb-logger runtime.
    ///
    /// This consumes the `UsbLogger` to ensure that only one instance of the logging
    /// runtime can be active for the life of the driver.
    async fn run(self) {
        let driver = self.driver;
        embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
    }
}

/// `PotentiometerManager` handles reading the analog value from the physical DHD dial.
///
/// It acts as a hardware sensor driver that periodically samples the ADC and translates
/// the raw voltage into a normalized float. By updating the `VolumeState`, it
/// provides a bridge between physical user interaction and the rest of the DHD's
/// logic, enabling the device to track knob movement in real-time.
struct PotentiometerManager {
    adc: Adc<'static, Async>,
    channel: Channel<'static>,
    state: &'static VolumeState,
    interval: Duration,
    bottom: f32,
    top: f32,
}

impl PotentiometerManager {
    /// Creates a new `PotentiometerManager` with the specified calibration and timing.
    ///
    /// * `adc`: The initialized ADC peripheral.
    /// * `channel`: The specific ADC channel (GP26) connected to the dial wiper.
    /// * `state`: The `VolumeState` to update with new readings.
    /// * `interval`: How often to sample the hardware.
    /// * `bottom`: Clipping level for the low end (clamped to 0.0 below this).
    /// * `top`: Clipping level for the high end (clamped to 1.0 above this).
    fn new(
        adc: Adc<'static, Async>,
        channel: Channel<'static>,
        state: &'static VolumeState,
        interval: Duration,
        bottom: f32,
        top: f32,
    ) -> Self {
        Self {
            adc,
            channel,
            state,
            interval,
            bottom,
            top,
        }
    }

    /// Continuously polls the ADC and updates the shared volume state.
    ///
    /// This loop converts the raw 12-bit ADC range into a normalized 0.0..1.0 range,
    /// applying the configured clipping to handle noise at the potentiometers' limits.
    async fn run(mut self) {
        let mut ticker = Ticker::every(self.interval);
        loop {
            if let Ok(raw_val) = self.adc.read(&mut self.channel).await {
                // RP2350 ADC is 12-bit (0-4095)
                let val = raw_val as f32 / 4095.0;

                // Apply clipping and normalize to 0..1 range within the clipped window
                let normalized = if val <= self.bottom {
                    0.0
                } else if val >= self.top {
                    1.0
                } else {
                    (val - self.bottom) / (self.top - self.bottom)
                };

                self.state.set(normalized);
            }
            ticker.next().await;
        }
    }
}

/// `Monitor` provides system observability and diagnostics for the DHD.
///
/// It periodically reads the shared volume state and reports it via the USB logging system.
/// This allows us to verify dial calibration and responsiveness in real-time
/// without impacting the high-frequency sampling loops.
struct Monitor {
    state: &'static VolumeState,
}

impl Monitor {
    /// Injects the shared state that this monitor will observe.
    fn new(state: &'static VolumeState) -> Self {
        Self { state }
    }

    /// Periodically logs the current volume level.
    async fn run(self) {
        // Delay to allow the USB device to enumerate on the host machine before we start logging.
        Timer::after_secs(1).await;
        info!("DHD (Dial Hifi Device) starting...");

        let mut ticker = Ticker::every(Duration::from_secs(1));
        loop {
            ticker.next().await;
            let current = self.state.get();
            info!("DHD Volume: {:.3}", current);
        }
    }
}

/// Embassy task wrapper for the USB Logger.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn usb_logger_task(logger: UsbLogger) {
    logger.run().await;
}

/// Embassy task wrapper for the Potentiometer polling loop.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn potentiometer_task(manager: PotentiometerManager) {
    manager.run().await;
}

/// Embassy task wrapper for the System Monitor.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn monitor_task(monitor: Monitor) {
    monitor.run().await;
}

/// The main entry point responsible for DHD system orchestration and dependency injection.
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // USB driver setup: The logger owns the hardware and is spawned first to provide output.
    let driver = Driver::new(p.USB, Irqs);
    let logger = UsbLogger::new(driver);
    spawner.spawn(usb_logger_task(logger).unwrap());

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

    // Dependency Injection: Component assembly.
    let pot_manager = PotentiometerManager::new(
        adc,
        channel,
        volume,
        Duration::from_millis(50),
        0.02,
        0.98,
    );
    let monitor = Monitor::new(volume);

    // Final hand-off to the async executor.
    spawner.spawn(potentiometer_task(pot_manager).unwrap());
    spawner.spawn(monitor_task(monitor).unwrap());

    // The main task yields control to the other tasks indefinitely.
    loop {
        Timer::after_secs(3600).await;
    }
}
