use embassy_time::Instant;
use crate::fader::{FaderState, interface::FaderInterface};
use crate::comms::CommsBus;

pub struct CalibrationService {
    fader: FaderInterface,
    fader_state: &'static FaderState,
    comms: &'static CommsBus,
    samples_buf: &'static mut [(Instant, u16)],
    hysteresis: u16,
}

/// Outcome of the boundaries calibration
pub struct Boundaries {
    pub min: u16,
    pub max: u16,
    pub speed_scale: f32,
}

impl CalibrationService {
    pub fn new(
        fader: FaderInterface, 
        fader_state: &'static FaderState, 
        comms: &'static CommsBus,
        samples: &'static mut [(Instant, u16)], 
        hysteresis: u16
    ) -> Self {
        Self { fader, fader_state, comms, samples_buf: samples, hysteresis }
    }

    pub async fn run_calibration(&mut self) {
        log::info!("Calibration starting...");

        let boundaries = self.find_boundaries(0.5).await;
        log::info!("Boundaries: {} - {}", boundaries.min, boundaries.max);
        log::info!("Speed Scale: {}", boundaries.speed_scale);

        let lowest_speed = self.find_lowest_speed().await;
        log::info!("Lowest speed: {}", lowest_speed);

        log::info!("Calibration complete.");
    }

    /// Explores the boundaries of the potentiometer
    ///
    /// First we push in the positive speed direction, so `a` should be at the
    /// maximum value that the potentiometer will read. The vice-versa. If this
    /// is inverted, we need to invert speed.
    async fn find_boundaries(&mut self, speed: f32) -> Boundaries {
        let mut a = self.fader.drive_until_stall(speed).await;
        let mut b = self.fader.drive_until_stall(-speed).await;
        let mut s = 1.0;

        if a < b {
            (a, b) = (b, a);
            s = -1.0;
        }

        Boundaries {
            max: a,
            min: b,
            speed_scale: s,
        }
    }

    /// The motor doesn't move beyond a given speed. Here we look for the
    /// lowest speed that manages to move the knob. We do this using a
    /// bisection algorithm, testing different values and seeing at which point
    /// it stops moving, with a precision of 0.01 (on the scale from 0 to 1).
    async fn find_lowest_speed(&mut self) -> f32 {
        let mut left = 0.0;
        let mut right = 1.0;

        loop {
            let mid = (left + right) / 2.0;
            if self.fader.moves_at_speed(mid).await {
                right = mid;
            } else {
                left = mid;
            }

            if right - left < 0.01 {
                return left;
            }
        }
    }
}
