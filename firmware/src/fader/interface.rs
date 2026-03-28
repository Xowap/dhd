use core::sync::atomic::Ordering;
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use crate::utils::math::circular_linear_regression;
use super::FaderState;

/// A handler to control the speed of the motorized fader.
///
/// The goal is to use the RAII pattern in order to make sure that the speed
/// goes back to zero once the caller is done.
pub struct SpeedController<'a> {
    pub fader: &'a mut FaderInterface,
}

impl<'a> SpeedController<'a> {
    pub fn new(fader: &'a mut FaderInterface) -> Self {
        Self { fader }
    }

    /// Configure the PWM to go at a speed between 0 and 1. The "direction" of
    /// the speed is unspecified and will depend on the calibration's outcome.
    /// Check the set_speed() instead if you want something usable.
    pub fn set_raw_speed(&mut self, speed: f32) {
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

        self.fader.pwm.set_config(&config);
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.set_raw_speed(speed * self.fader.state.speed_scale);
    }
}

impl<'a> Drop for SpeedController<'a> {
    fn drop(&mut self) {
        self.set_raw_speed(0.0);
    }
}

/// High-level handle for the motorized fader hardware.
pub struct FaderInterface {
    pub pwm: Pwm<'static>,
    pub state: &'static FaderState,
}

impl FaderInterface {
    pub fn new(pwm: Pwm<'static>, state: &'static FaderState) -> Self {
        Self { pwm, state }
    }

    /// Proxy to to obtain the raw position of the potentiometer
    pub fn get_raw_pos(&self) -> u16 {
        self.state.last_raw_adc.load(Ordering::Relaxed) as u16
    }

    /// The speed controller allows to control the motor's speed while making
    /// sure that it is reset to zero after use (using the RAII pattern)
    pub fn get_speed_controller(&mut self) -> SpeedController<'_> {
        SpeedController::new(self)
    }

    /// The idea is to set the speed and wait until the moving trend stops.
    /// Then we shut down the motor (which creates lots of noise) and measure
    /// the stable position.
    pub async fn drive_until_stall(&mut self, speed: f32) -> u16 {
        let mut speed_controller = self.get_speed_controller();
        speed_controller.set_raw_speed(speed);

        let mut positions = [0u16; 101];
        let mut i = 0;

        loop {
            speed_controller.fader.state.sig_raw_changed.wait().await;
            positions[i % positions.len()] = speed_controller.fader.get_raw_pos();

            if i >= positions.len() {
                match circular_linear_regression(&positions, i) {
                    None => {}
                    Some((slope, _)) => {
                        if slope.abs() < 1.0 {
                            speed_controller.set_raw_speed(0.0);
                            break;
                        }
                    }
                }
            }

            i += 1;
        }

        for i in 0..positions.len() {
            speed_controller.fader.state.sig_raw_changed.wait().await;
            positions[i] = speed_controller.fader.get_raw_pos();
        }

        positions.sort_unstable();

        return positions[(positions.len() - 1) / 2];
    }
}
