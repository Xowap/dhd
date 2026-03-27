use core::sync::atomic::Ordering;
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_time::{Duration, Instant, Timer};
use super::FaderState;

/// High-level handle for the motorized fader hardware.
pub struct FaderInterface {
    pwm: Pwm<'static>,
    state: &'static FaderState,
}

impl FaderInterface {
    pub fn new(pwm: Pwm<'static>, state: &'static FaderState) -> Self {
        Self { pwm, state }
    }

    pub fn set_motor_speed(&mut self, speed: f32) {
        let speed = speed.clamp(-1.0, 1.0);
        let mut config = PwmConfig::default();
        config.top = 10000;
        if speed > 0.0 {
            let duty = (speed * 10000.0) as u16;
            config.compare_a = 10000;
            config.compare_b = 10000 - duty;
        } else if speed < 0.0 {
            let duty = (-speed * 10000.0) as u16;
            config.compare_a = 10000 - duty;
            config.compare_b = 10000;
        } else {
            config.compare_a = 10000;
            config.compare_b = 10000;
        }
        self.pwm.set_config(&config);
    }

    pub fn get_raw_pos(&self) -> u16 {
        self.state.last_raw_adc.load(Ordering::Relaxed) as u16
    }

    pub async fn drive_until_stall(&mut self, speed: f32) -> u16 {
        self.set_motor_speed(speed);
        let mut last_pos = self.get_raw_pos();
        let mut moving = false;
        let mut timer = Instant::now();
        let start = Instant::now();

        loop {
            Timer::after_millis(20).await;
            let current = self.get_raw_pos();
            let delta = (current as i32 - last_pos as i32).abs();

            if !moving {
                if delta > 2 {
                    moving = true;
                    timer = Instant::now();
                } else if start.elapsed() > Duration::from_millis(300) { break; }
            } else {
                if delta > 2 {
                    timer = Instant::now();
                } else if timer.elapsed() > Duration::from_millis(150) { break; }
            }

            last_pos = current;
            if start.elapsed() > Duration::from_secs(4) { break; }
        }
        self.set_motor_speed(0.0);
        Timer::after_millis(50).await;
        self.get_raw_pos()
    }
}
