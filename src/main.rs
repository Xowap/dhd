//! This example demonstrates a dependency-injection-based architecture for an embedded system.
//!
//! Rather than relying on global state or hard-coded peripheral access, this design uses
//! the `main` entry point to initialize hardware and shared resources. These resources are
//! then explicitly injected into distinct component structs, ensuring that every part of
//! the system has clear ownership and access only to the data it requires to function.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};
use log::info;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_time::{Duration, Instant, Ticker, Timer};
use micromath::F32Ext;
use static_cell::StaticCell;
use {panic_halt as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// Program metadata for `picotool info`.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"LedManager DI Example"),
    embassy_rp::binary_info::rp_program_description!(
        c"LedManager with Dependency Injection and USB Logging"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

/// The `LedManager` acts as the single source of truth for the system's target LED state.
///
/// It facilitates communication between the controller (which determines the desired brightness)
/// and the blinker (which applies that brightness). By using atomic operations, it allows
/// these two tasks to interact safely without the overhead of a mutex, keeping the control
/// loop responsive.
struct LedManager {
    /// Intensity stored as parts per million (0 to 1,000,000).
    /// Atomic storage ensures we can update and read the intensity across different
    /// execution contexts without data races.
    intensity: AtomicU32,
}

impl LedManager {
    /// Creates a new `LedManager` instance in its default state (LED off).
    const fn new() -> Self {
        Self {
            intensity: AtomicU32::new(0),
        }
    }

    /// Sets the desired LED intensity.
    ///
    /// This is typically called by a "driver" or "controller" component to communicate
    /// a new state to the hardware-facing tasks.
    fn set_intensity(&self, val: f32) {
        let val = (val.clamp(0.0, 1.0) * 1_000_000.0) as u32;
        self.intensity.store(val, Ordering::Relaxed);
    }

    /// Returns the current desired LED intensity.
    ///
    /// This is used by the blinker task to determine its duty cycle and by the monitor
    /// task to report current status.
    fn get_intensity(&self) -> f32 {
        self.intensity.load(Ordering::Relaxed) as f32 / 1_000_000.0
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

/// `LedBlinker` is the hardware driver responsible for physical LED control.
///
/// It implements a software-based PWM (Pulse Width Modulation) loop. By injecting the
/// `LedManager` reference, it remains decoupled from the logic that decides *how* bright
/// the LED should be, focusing entirely on *how* to achieve that brightness on the pin.
struct LedBlinker {
    led: Output<'static>,
    manager: &'static LedManager,
}

impl LedBlinker {
    /// Pairs a physical GPIO pin with a shared `LedManager` state.
    fn new(led: Output<'static>, manager: &'static LedManager) -> Self {
        Self { led, manager }
    }

    /// Executes the software PWM loop.
    ///
    /// This runs at a high frequency to simulate analog dimming. It continuously polls
    /// the injected `LedManager` for the current target intensity.
    async fn run(mut self) {
        loop {
            let intensity = self.manager.get_intensity();

            if intensity <= 0.0 {
                self.led.set_low();
                Timer::after_millis(10).await;
            } else if intensity >= 1.0 {
                self.led.set_high();
                Timer::after_millis(10).await;
            } else {
                let on_time_us = (intensity * 1000.0) as u64; 
                let off_time_us = 1000 - on_time_us;

                if on_time_us > 0 {
                    self.led.set_high();
                    Timer::after_micros(on_time_us).await;
                }
                if off_time_us > 0 {
                    self.led.set_low();
                    Timer::after_micros(off_time_us).await;
                }
            }
        }
    }
}

/// `DimController` manages the behavioral logic of the LED.
///
/// It calculates a sinusoidal intensity curve over time. By injecting the `LedManager`,
/// this component doesn't need to know anything about GPIO pins or PWM; it simply
/// updates the shared state with its calculated values.
struct DimController {
    manager: &'static LedManager,
}

impl DimController {
    /// Injects the shared state that this controller will manipulate.
    fn new(manager: &'static LedManager) -> Self {
        Self { manager }
    }

    /// Drives the intensity following a mathematical sinusoid.
    ///
    /// This task updates the intensity at a fixed frequency (50Hz), providing smooth
    /// transitions independent of the LED hardware implementation.
    async fn run(self) {
        let mut ticker = Ticker::every(Duration::from_millis(20));
        let start = Instant::now();
        let period_secs = 5.0;

        loop {
            let elapsed = start.elapsed();
            let elapsed_secs = elapsed.as_millis() as f32 / 1000.0;
            let sin_val = (elapsed_secs * 2.0 * core::f32::consts::PI / period_secs).sin();
            let intensity = (sin_val + 1.0) / 2.0;
            
            self.manager.set_intensity(intensity);
            
            ticker.next().await;
        }
    }
}

/// `Monitor` provides system observability and diagnostics.
///
/// It periodically reads the shared system state and reports it via the logging system.
/// This allows us to verify the system behavior without impacting the core control loop.
struct Monitor {
    manager: &'static LedManager,
}

impl Monitor {
    /// Injects the shared state that this monitor will observe.
    fn new(manager: &'static LedManager) -> Self {
        Self { manager }
    }

    /// Periodically logs the current system intensity.
    async fn run(self) {
        let mut ticker = Ticker::every(Duration::from_secs(1));
        loop {
            ticker.next().await;
            let intensity = self.manager.get_intensity();
            info!("Current LED intensity: {}", intensity);
        }
    }
}

/// Embassy task wrapper for the USB Logger.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn usb_logger_task(logger: UsbLogger) {
    logger.run().await;
}

/// Embassy task wrapper for the LED Blink loop.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn led_blinker_task(blinker: LedBlinker) {
    blinker.run().await;
}

/// Embassy task wrapper for the Sinusoidal Intensity Controller.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn dim_controller_task(controller: DimController) {
    controller.run().await;
}

/// Embassy task wrapper for the System Monitor.
/// This exists to adapt the struct-based runner to the executor's task requirements.
#[embassy_executor::task]
async fn monitor_task(monitor: Monitor) {
    monitor.run().await;
}

/// The main entry point responsible for system orchestration and dependency injection.
///
/// Here, we initialize the hardware, create shared system-wide resources (like the `LedManager`),
/// and assemble the components by injecting their dependencies. This centralized setup
/// ensures that the application architecture is visible at a glance and that no component
/// creates its own hidden dependencies.
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // USB driver setup: The logger owns the hardware and is spawned first to provide output.
    let driver = Driver::new(p.USB, Irqs);
    let logger = UsbLogger::new(driver);
    spawner.spawn(usb_logger_task(logger).unwrap());

    // LedManager is allocated in a StaticCell to provide a 'static reference that can
    // be shared safely between multiple tasks.
    static LED_MANAGER: StaticCell<LedManager> = StaticCell::new();
    let manager = LED_MANAGER.init(LedManager::new());

    // Delay to allow the USB device to enumerate on the host machine.
    Timer::after_millis(500).await;
    
    info!("LedManager Struct-based DI started");

    // Initialize hardware pin as a dependency.
    let led = Output::new(p.PIN_25, Level::Low);

    // Dependency Injection: Component assembly.
    // We explicitly pass shared state and hardware ownership to the relevant objects.
    let blinker = LedBlinker::new(led, manager);
    let dim_controller = DimController::new(manager);
    let monitor = Monitor::new(manager);

    // Final hand-off to the async executor.
    spawner.spawn(led_blinker_task(blinker).unwrap());
    spawner.spawn(dim_controller_task(dim_controller).unwrap());
    spawner.spawn(monitor_task(monitor).unwrap());

    // The main task yields control to the other tasks indefinitely.
    loop {
        Timer::after_secs(3600).await;
    }
}
