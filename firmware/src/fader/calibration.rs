use common::pid::PidController;
use serde::{Deserialize, Serialize};

use crate::fader::interface::FaderInterface;

pub struct CalibrationService {
    fader: Option<FaderInterface>,
}

/// Outcome of the boundaries calibration
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

/// Outcome of the physical boundaries calibration
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PhysicalCalibration {
    /// This way we know what are the min/max values to use on the
    /// potentiometer (to convert from the 0-100% scale to the physical scale
    /// and vice-versa).
    pub boundaries: Boundaries,

    /// Lowest speed at which the knob will move (below this, the power given
    /// to the motor will not be enough).
    pub lowest_speed: f32,
}

/// Outcome of the PID auto-calibration
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PidCalibration {
    pub kp: f32,
    pub ki: f32,
    pub kd: f32,
    pub alpha: f32,
    pub deadband: f32,
}

/// Outcome of the calibration process
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub physical: PhysicalCalibration,
    pub pid: PidCalibration,
    /// Whether the volume scale is inverted (hardware 100% = volume 0%).
    /// Defaults to false for backward compatibility with existing stored data.
    #[serde(default)]
    pub inverted: bool,
}

impl CalibrationService {
    pub fn new(fader: FaderInterface) -> Self {
        Self { fader: Some(fader) }
    }

    pub fn take_fader(&mut self) -> FaderInterface {
        self.fader.take().expect("Fader already taken")
    }

    pub fn return_fader(&mut self, fader: FaderInterface) {
        self.fader = Some(fader);
    }

    /// Explores the physical boundaries and stiction of the fader.
    pub async fn run_physical_calibration(&mut self) -> PhysicalCalibration {
        log::info!("Physical calibration starting...");
        let fader = self.fader.as_mut().expect("Fader missing");

        let boundaries = Self::find_boundaries(fader, 0.5).await;
        log::debug!("Boundaries: {} - {}", boundaries.min, boundaries.max);
        log::debug!("Speed Scale: {}", boundaries.speed_scale);

        let lowest_speed = Self::find_lowest_speed(fader).await;
        log::debug!("Lowest speed: {}", lowest_speed);

        PhysicalCalibration {
            boundaries,
            lowest_speed,
        }
    }

    /// Runs the PID auto-calibration.
    /// This should be called AFTER the physical calibration has been applied
    /// to the fader state.
    pub async fn run_pid_calibration(&mut self) -> PidCalibration {
        log::info!("PID calibration starting...");
        let fader_owned = self.fader.take().expect("Fader missing");

        log::debug!("Starting PID autotune...");
        let mut pid = PidController::<_, 50>::new(fader_owned, 0.0, 0.0, 0.0, 0.0);
        pid.calibrate().await;

        let res = PidCalibration {
            kp: pid.kp,
            ki: pid.ki,
            kd: pid.kd,
            alpha: pid.alpha,
            deadband: pid.deadband_frac,
        };

        log::debug!("Final Kp: {:.4}", res.kp);
        log::debug!("Final Ki: {:.4}", res.ki);
        log::debug!("Final Kd: {:.4}", res.kd);
        log::debug!("Final alpha: {:.4}", res.alpha);
        log::debug!("Final deadband: {:.4}", res.deadband);

        self.fader = Some(pid.hardware);
        res
    }

    /// Explores the boundaries of the potentiometer
    ///
    /// First we push in the positive speed direction, so `a` should be at the
    /// maximum value that the potentiometer will read. The vice-versa. If this
    /// is inverted, we need to invert speed.
    async fn find_boundaries(fader: &mut FaderInterface, speed: f32) -> Boundaries {
        let mut a = fader.drive_until_stall(speed).await;
        let mut b = fader.drive_until_stall(-speed).await;
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
    async fn find_lowest_speed(fader: &mut FaderInterface) -> f32 {
        let mut left = 0.0;
        let mut right = 1.0;

        loop {
            let mid = (left + right) / 2.0;
            if fader.moves_at_speed(mid).await {
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
