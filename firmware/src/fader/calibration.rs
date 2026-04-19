use crate::fader::interface::FaderInterface;

pub struct CalibrationService {
    fader: FaderInterface,
}

/// Outcome of the boundaries calibration
#[derive(Clone, Copy, Debug)]
pub struct Boundaries {
    /// Minimum value that you will read on the potentiometer
    pub min: u16,

    /// Maximum value that you will read on the potentiometer
    pub max: u16,

    /// Can be either 1 or -1, which tells you in which direction the knob
    /// moves based on the speed you send. This is because knowing which wire
    /// from the motor is which is a fucking pain in the ass so I much prefer
    /// to have this figured at runtime than to re-solder the damn fucking
    /// thing. If positive, positive speed increases the value. If negative,
    /// negative speed increases the value.
    pub speed_scale: f32,
}

impl Boundaries {
    /// Transforms the raw ADC value into a 0.0-1.0 value, clipped according to
    /// the boundaries.
    pub fn interpolate(&self, val: u16) -> f32 {
        if self.max <= self.min {
            return 0.0;
        }

        let val = val.clamp(self.min, self.max);
        let range = (self.max - self.min) as f32;

        (val - self.min) as f32 / range
    }
}

/// Outcome of the calibration process
#[derive(Clone, Copy, Debug)]
pub struct CalibrationResult {
    /// This way we know what are the min/max values to use on the
    /// potentiometer (to convert from the 0-100% scale to the physical scale
    /// and vice-versa).
    pub boundaries: Boundaries,

    /// Lowest speed at which the knob will move (below this, the power given
    /// to the motor will not be enough).
    pub lowest_speed: f32,
}

impl CalibrationResult {
    pub fn new() -> Self {
        Self {
            boundaries: Boundaries {
                min: 0,
                max: 0,
                speed_scale: 0.0,
            },
            lowest_speed: 0.0,
        }
    }
}

impl CalibrationService {
    pub fn new(fader: FaderInterface) -> Self {
        Self { fader }
    }

    /// Runs the calibration process, which gives the rest of the program a
    /// clear knowledge of the system's physical parameters
    pub async fn run_calibration(&mut self) -> Option<CalibrationResult> {
        let mut out = CalibrationResult::new();
        log::info!("Calibration starting...");

        out.boundaries = self.find_boundaries(0.5).await;
        log::info!(
            "Boundaries: {} - {}",
            out.boundaries.min,
            out.boundaries.max
        );
        log::info!("Speed Scale: {}", out.boundaries.speed_scale);

        out.lowest_speed = self.find_lowest_speed().await;
        log::info!("Lowest speed: {}", out.lowest_speed);

        Some(out)
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
