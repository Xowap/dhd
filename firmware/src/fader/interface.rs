use super::FaderState;
use crate::utils::math::circular_linear_regression;
use common::pid::PidHardware;
use core::sync::atomic::Ordering;
use embassy_rp::pwm::{Config as PwmConfig, Pwm};

/// High-level handle for the motorized fader hardware.
pub struct FaderInterface {
    pub pwm: Pwm<'static>,
    pub state: &'static FaderState,
}

impl FaderInterface {
    pub fn new(pwm: Pwm<'static>, state: &'static FaderState) -> Self {
        Self { pwm, state }
    }

    /// Set the motor speed directly.
    /// Uses Slow Decay (Brake mode) to ensure the fader stops immediately and
    /// precisely when the PID loop commands 0.0 or during PWM off-cycles.
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

        self.pwm.set_config(&config);
    }

    /// Proxy to to obtain the raw position of the potentiometer
    pub fn get_raw_pos(&self) -> u16 {
        self.state.last_raw_adc.load(Ordering::Relaxed) as u16
    }

    /// The idea is to set the speed and wait until the moving trend stops.
    /// Then we shut down the motor (which creates lots of noise) and measure
    /// the stable position.
    pub async fn drive_until_stall(&mut self, speed: f32) -> u16 {
        log::info!("Driving until stall: speed={:.2}", speed);
        self.set_raw_speed(speed);

        let mut positions = [0u16; 101];
        let mut i = 0;

        loop {
            self.state.sig_raw_changed.wait().await;
            positions[i % positions.len()] = self.get_raw_pos();

            if i >= positions.len() {
                match circular_linear_regression(&positions, i) {
                    None => {}
                    Some((slope, _)) => {
                        if slope.abs() < 1.0 {
                            self.set_raw_speed(0.0);
                            break;
                        }
                    }
                }
            }

            i += 1;
        }

        for pos in &mut positions {
            self.state.sig_raw_changed.wait().await;
            *pos = self.get_raw_pos();
        }

        self.set_raw_speed(0.0);
        positions.sort_unstable();

        positions[(positions.len() - 1) / 2]
    }

    /// First rams the knob into one end at 0.5 speed (which we consider safe)
    /// and then we move it in the other direction for a few samples. If the
    /// samples don't seem to be trending, we consider that the knob is stuck
    /// and thus that this speed is not a "moving" speed (when it's too low).
    pub async fn moves_at_speed(&mut self, speed: f32) -> bool {
        self.drive_until_stall(speed.signum() * -0.5).await;
        let mut samples = [0u16; 20];

        self.set_raw_speed(speed);

        for sample in &mut samples {
            self.state.sig_raw_changed.wait().await;
            *sample = self.get_raw_pos();
        }

        self.set_raw_speed(0.0);

        match circular_linear_regression(&samples, samples.len()) {
            Some((slope, _)) => slope.abs() > 1.0,
            None => false,
        }
    }
}

impl PidHardware for FaderInterface {
    async fn read_measurement(&mut self) -> f32 {
        self.state.sig_raw_changed.wait().await;
        let raw = self.get_raw_pos();
        self.state
            .calibration
            .lock()
            .await
            .physical
            .boundaries
            .interpolate(raw)
    }

    async fn write_output(&mut self, value: f32) {
        let speed_scale = self
            .state
            .calibration
            .lock()
            .await
            .physical
            .boundaries
            .speed_scale;
        self.set_raw_speed(value * speed_scale);
    }

    fn gain_range(&self) -> (f32, f32) {
        let cal = self
            .state
            .calibration
            .try_lock()
            .expect("Calibration locked during gain_range");
        (cal.physical.lowest_speed, 1.0)
    }

    fn measurement_range(&self) -> (f32, f32) {
        (0.0, 1.0)
    }

    fn midpoint(&self) -> f32 {
        0.5
    }

    async fn home(&mut self) {
        let speed_scale = self
            .state
            .calibration
            .lock()
            .await
            .physical
            .boundaries
            .speed_scale;
        self.drive_until_stall(-0.5 * speed_scale).await;
    }

    fn on_target_reached(&mut self) {
        log::info!("Target reached");
    }

    fn on_calibrate_progress(&mut self, msg: &str) {
        log::info!("{}", msg);
    }
}
