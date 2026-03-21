//! This example test the RP Pico on board LED with a LedManager.
//!
//! It uses software PWM to control intensity and an async task to
//! vary it following a sinusoid.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};
use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio;
use embassy_time::{Duration, Instant, Ticker, Timer};
use gpio::{Level, Output};
use micromath::F32Ext;
use {defmt_rtt as _, panic_probe as _};

// Program metadata for `picotool info`.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"LedManager Example"),
    embassy_rp::binary_info::rp_program_description!(
        c"LedManager with software PWM and sinusoid intensity variation"
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
///
/// Note: With async timers, the resolution is limited to 1 microsecond (hardware timer)
/// and the actual switching speed is limited by executor overhead (approx 10-20us).
/// For 10ns resolution, hardware PWM or PIO would be required.
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
            // We use a 1000us (1ms) cycle for software PWM.
            // This gives a 1kHz frequency, which is smooth for the eye.
            let on_time_us = (intensity * 10000.0) as u64;
            let off_time_us = 10000 - on_time_us;

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
    let period_secs = 10.0;

    loop {
        let elapsed_secs = start.elapsed().as_millis() as f32 / 1000.0;
        
        // Calculate sinusoid: result is in [-1, 1]
        let sin_val = (elapsed_secs * 2.0 * core::f32::consts::PI / period_secs).sin();
        
        // Map [-1, 1] to [0, 1]
        let intensity = (sin_val + 1.0) / 2.0;
        
        LED_MANAGER.set_intensity(intensity);
        
        ticker.next().await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    
    info!("LedManager started");

    let led = Output::new(p.PIN_25, Level::Low);

    // Spawn the tasks
    spawner.spawn(led_blinker_task(led)).unwrap();
    spawner.spawn(intensity_sinusoid_task()).unwrap();

    // Main task stays alive
    loop {
        Timer::after_secs(3600).await;
    }
}
