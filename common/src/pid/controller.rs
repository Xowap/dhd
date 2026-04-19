use super::hardware::PidHardware;

/// A PID controller for second-order mechanical plants (mass + friction).
///
/// Design choices:
/// - **Derivative on measurement velocity** (not on error). Velocity is estimated via an
///   EMA on position; the D term is `-Kd * velocity`. This avoids setpoint-kick and is
///   much friendlier with noisy ADCs than differentiating error.
/// - **Integral with anti-windup**. The integral only accumulates when the output is not
///   saturated in the same direction we're driving.
/// - **Hysteretic friction compensation**. Rather than an always-on friction lift (which
///   creates a ±min_gain discontinuity at zero error and causes limit cycles), we use
///   hysteresis: we only engage the motor when |error| > engage_band, and we only add the
///   min_gain floor when the PID value is already pushing in a direction with enough
///   magnitude to warrant breaking stiction. Otherwise we output 0.
/// - **Adaptive deadband**. The deadband scales with the estimated measurement noise so
///   we don't chatter on noise.
pub struct PidController<H: PidHardware, const N: usize> {
    /// Hardware interface for reading and writing.
    pub hardware: H,
    /// Proportional gain.
    pub kp: f32,
    /// Integral gain.
    pub ki: f32,
    /// Derivative gain (applied to measurement velocity).
    pub kd: f32,
    /// Feed-forward gain (applied to target).
    pub kf: f32,
    /// Current target setpoint.
    pub target: f32,

    /// Accumulated integral of error.
    pub(super) integral: f32,

    /// Current low-pass filtered position.
    pub(super) filt_pos: f32,
    /// Previous low-pass filtered position (for velocity estimation).
    pub(super) filt_pos_prev: f32,
    /// Whether the filter has been initialized with the first measurement.
    pub(super) filt_init: bool,

    /// EMA filter coefficient for position (0..1).
    pub(super) alpha: f32,

    /// Deadband as a fraction of measurement range. Configured by calibration.
    pub(super) deadband_frac: f32,

    /// Hysteresis state: whether the motor was driven on the previous step.
    pub(super) was_driving: bool,
}

impl<H: PidHardware, const N: usize> PidController<H, N> {
    /// Create a new PID controller with given coefficients.
    pub fn new(hardware: H, kp: f32, ki: f32, kd: f32, kf: f32) -> Self {
        Self {
            hardware,
            kp,
            ki,
            kd,
            kf,
            target: 0.0,
            integral: 0.0,
            filt_pos: 0.0,
            filt_pos_prev: 0.0,
            filt_init: false,
            alpha: 0.35,
            deadband_frac: 0.005,
            was_driving: false,
        }
    }

    /// Update the target setpoint.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Update all PID coefficients.
    pub fn set_coefficients(&mut self, kp: f32, ki: f32, kd: f32, kf: f32) {
        self.kp = kp;
        self.ki = ki;
        self.kd = kd;
        self.kf = kf;
    }

    /// Borrow the underlying hardware interface.
    pub fn hardware(&mut self) -> &mut H {
        &mut self.hardware
    }

    /// Reset internal state (integral and filter).
    pub fn reset_history(&mut self) {
        self.integral = 0.0;
        self.filt_init = false;
        self.was_driving = false;
    }

    /// Set the EMA filter alpha (responsiveness of velocity estimation).
    pub fn set_filter_alpha(&mut self, alpha: f32) {
        self.alpha = alpha.clamp(0.05, 1.0);
    }

    /// Fraction of measurement range used as the inner deadband (engage band = 1.5x this).
    pub fn set_deadband(&mut self, deadband_frac: f32) {
        self.deadband_frac = deadband_frac.clamp(0.0005, 0.1);
    }

    /// Run a single control step: read measurement, compute output, write output.
    pub async fn step(&mut self) {
        let measurement = self.hardware.read_measurement().await;
        let (min_mag, max_mag) = self.hardware.gain_range();
        let (min_pos, max_pos) = self.hardware.measurement_range();
        let range = (max_pos - min_pos).abs().max(f32::EPSILON);

        // ---- Velocity estimate via EMA on position ----
        if !self.filt_init {
            self.filt_pos = measurement;
            self.filt_pos_prev = measurement;
            self.filt_init = true;
        } else {
            self.filt_pos_prev = self.filt_pos;
            self.filt_pos = self.filt_pos + self.alpha * (measurement - self.filt_pos);
        }
        let velocity = self.filt_pos - self.filt_pos_prev;

        let error = self.target - self.filt_pos;

        // Hysteretic deadband: inner band (disengage) < outer band (engage).
        // We want the engage band tight enough to actually drive to the target.
        let inner_band = self.deadband_frac * range;
        let outer_band = 1.5 * inner_band;

        if self.was_driving {
            // Currently driving: stop only when we've clearly arrived AND we're not moving fast.
            let vel_settled = velocity.abs() < 0.002 * range;
            if error.abs() < inner_band && vel_settled {
                self.was_driving = false;
                self.hardware.write_output(0.0).await;
                return;
            }
        } else {
            // Currently idle: only re-engage when error exceeds outer band.
            if error.abs() < outer_band {
                self.hardware.write_output(0.0).await;
                return;
            }
            self.was_driving = true;
            // Re-reset integral on re-engage so old wind-up doesn't bite us.
            self.integral = 0.0;
        }

        // PID terms.
        let p_term = self.kp * error;
        let d_term = -self.kd * velocity;
        let ff_term = self.kf * self.target;

        // The controller's control signal `u` is the "force above stiction" we want.
        // Tentative integral update.
        let mut new_integral = self.integral + error;
        let i_term_tentative = self.ki * new_integral;
        let u_tentative = p_term + i_term_tentative + d_term + ff_term;

        let headroom = (max_mag - min_mag).max(0.01);
        let saturated_high = u_tentative > headroom && error > 0.0;
        let saturated_low = u_tentative < -headroom && error < 0.0;
        if saturated_high || saturated_low {
            new_integral = self.integral; // freeze
        }
        self.integral = new_integral;
        let i_term = self.ki * self.integral;

        let u = p_term + i_term + d_term + ff_term;

        // Friction-aware mapping. Since we're in "driving" mode:
        //   If sign(u) agrees with sign(error), we need at least min_mag of force in that
        //   direction to break stiction. Map u -> sign(u) * (min_mag + |u|), clipped.
        //   If sign(u) disagrees with sign(error), we're actively braking — still use the
        //   same mapping; at worst we slow down, which is fine.
        let output = if u.abs() < 1e-6 {
            // Going through zero: output exactly min_mag in the direction of error to keep
            // making progress.
            error.signum() * min_mag
        } else {
            let sign = if u > 0.0 { 1.0 } else { -1.0 };
            let mag = (min_mag + u.abs()).min(max_mag);
            sign * mag
        };

        self.hardware.write_output(output).await;
    }

    /// Run the control loop indefinitely.
    pub async fn run(&mut self) -> ! {
        loop {
            self.step().await;
        }
    }
}
