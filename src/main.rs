//! This example test the RP Pico on board LED with a LedManager.
//! It logs over USB Serial using the log crate and embassy-usb-logger.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};
use log::info;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_time::{Duration, Instant, Ticker, Timer};
use gpio::{Level, Output};
use micromath::F32Ext;
use {panic_halt as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

// Program metadata for `picotool info`.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"LedManager Example"),
    embassy_rp::binary_info::rp_program_description!(
        c"LedManager with software PWM and USB Logging via log and embassy-usb-logger"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

/// LedManager handles the intensity state of the LED.
struct LedManager {
    /// Intensity stored as parts per million (0 to 1,000,000)
    intensity: AtomicU32,
}

impl LedManager {
    const fn new() -> Self {
        Self {
            intensity: AtomicU32::new(0),
        }
    }

    /// Set intensity between 0.0 and 1.0
    fn set_intensity(&self, xxx: f32) {
        let val = (xxx.clamp(0.0, 1.0) * 1_000_000.0) as u32;
        self.intensity.store(val, Ordering::Relaxed);
    }

    /// Get intensity as a float between 0.0 and 1.0
    fn get_intensity(&self) -> f32 {
        self.intensity.load(Ordering::Relaxed) as f32 / 1_000_000.0
    }
}

/// Global LedManager instance
static LED_MANAGER: LedManager = LedManager::new();

/// Task that blinks the LED as fast as possible to simulate PWM.
#[embassy_executor::task]
async fn led_blinker_task(mut led: Output<'static>) {
    loop {
        let intensity = LED_MANAGER.get_intensity();

        if intensity <= 0.0 {
            led.set_low();
            Timer::after_millis(10).await;
        } else if intensity >= 1.0 {
            led.set_high();
            Timer::after_millis(10).await;
        } else {
            let on_time_us = (intensity * 1000.0) as u64; 
            let off_time_us = 1000 - on_time_us;

            if on_time_us > 0 {
                led.set_high();
                Timer::after_micros(on_time_us).await;
            }
            if off_time_us > 0 {
                led.set_low();
                Timer::after_micros(off_time_us).await;
            }
        }
    }
}

/// Task that changes the intensity slowly following a sinusoid.
#[embassy_executor::task]
async fn intensity_sinusoid_task() {
    let mut ticker = Ticker::every(Duration::from_millis(20));
    let start = Instant::now();
    let period_secs = 5.0;

    loop {
        let elapsed = start.elapsed();
        let elapsed_secs = elapsed.as_millis() as f32 / 1000.0;
        let sin_val = (elapsed_secs * 2.0 * core::f32::consts::PI / period_secs).sin();
        let intensity = (sin_val + 1.0) / 2.0;
        
        LED_MANAGER.set_intensity(intensity);
        
        ticker.next().await;
    }
}

/// Task that logs the current intensity once per second.
#[embassy_executor::task]
async fn monitor_task() {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        ticker.next().await;
        let intensity = LED_MANAGER.get_intensity();
        info!("Current LED intensity: {}", intensity);
    }
}

#[embassy_executor::task]
async fn usb_logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // USB driver setup
    let driver = Driver::new(p.USB, Irqs);
    
    // Spawn USB logger task
    spawner.spawn(usb_logger_task(driver).expect("failed to create usb logger task token"));

    // Give the USB logger a moment to initialize before its first log
    Timer::after_millis(500).await;
    
    info!("LedManager started via USB Console");

    let led = Output::new(p.PIN_25, Level::Low);

    // Spawn the functional tasks
    spawner.spawn(led_blinker_task(led).expect("failed to create led task token"));
    spawner.spawn(intensity_sinusoid_task().expect("failed to create sinusoid task token"));
    spawner.spawn(monitor_task().expect("failed to create monitor task token"));

    loop {
        Timer::after_secs(3600).await;
    }
}
