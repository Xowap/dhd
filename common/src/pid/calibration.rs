use super::controller::PidController;
use super::hardware::PidHardware;
use core::fmt::Write;

// --- Calibration Constants & Assumptions ---

/// Number of samples to collect when measuring the stationary noise floor.
/// 24 samples at 2ms/sample = 48ms of observation.
const NOISE_SAMPLES: u32 = 24;

/// During plant ID, how many samples to drive the motor open-loop to observe its step response.
/// 100 samples = 200ms of acceleration.
const ID_DRIVE_SAMPLES: usize = 100;

/// Number of samples to wait (coast) after open-loop drive before returning control.
/// Allows the motor to naturally come to a halt.
const ID_COAST_SAMPLES: u32 = 40;

/// During plant ID, the fraction of the terminal velocity used to define the mechanical time constant (\tau).
/// 63% is the standard definition for a 1st order low-pass filter step response.
const TAU_VELOCITY_FRACTION: f32 = 0.63;

/// The minimum allowable candidate bandwidth (omega_n) in rad/sample.
const OMEGA_MIN: f32 = 0.04;
/// The maximum allowable candidate bandwidth (omega_n) in rad/sample.
const OMEGA_MAX: f32 = 0.25;

/// Number of steps in the initial coarse bandwidth search.
const COARSE_GRID_STEPS: u32 = 6;
/// Number of steps in the refined bandwidth search around the coarse winner.
const FINE_GRID_STEPS: u32 = 3;

/// The target damping ratio for the PID loop.
/// Set heavily overdamped (> 1.0) to prevent bouncing at the end of travel.
const TARGET_ZETA: f32 = 1.4;

/// Upper limit for Derivative Gain to prevent extreme jitter and noise amplification.
const MAX_KD: f32 = 40.0;
/// Bounds for Proportional Gain.
const KP_CLAMP: (f32, f32) = (0.1, 20.0);
/// Bounds for Integral Gain.
const KI_CLAMP: (f32, f32) = (0.0, 5.0);

/// Duration of a step-test trial in samples.
/// 150 samples = 300ms, which is enough time for the bandwidths we are testing to settle.
const TRIAL_HORIZON: u32 = 150;
/// Error bound (as fraction of total range) considered "settled". 0.02 = 2%.
const SETTLE_BAND: f32 = 0.02;
/// Number of samples to observe the motor coasting after shutting off at the end of a trial,
/// heavily penalizing momentum-induced drifting.
const COAST_OBSERVATION_SAMPLES: u32 = 20;

/// Result of a single step-test trial.
#[derive(Clone, Copy)]
struct TrialScore {
    /// Integral of absolute error (smaller is better).
    iae: f32,
    /// Maximum overshoot observed (as fraction of step size).
    overshoot: f32,
    /// Whether the system reached and stayed within the 2% settle band.
    settled: bool,
    /// The sample index at which the system first entered the settle band.
    settle_sample: u32,
    /// The final absolute error at the end of the trial (as fraction of range).
    final_err: f32,
}

impl TrialScore {
    /// Combined scalar score used for optimization. Smaller is better.
    /// Balances overshoot, settling time, and final error using aggressive penalty multipliers.
    fn scalar(&self) -> f32 {
        let overshoot_pen = if self.overshoot > 0.02 {
            (self.overshoot - 0.02) * 10000.0
        } else {
            0.0
        };
        let settle_pen = if self.settled {
            self.settle_sample as f32 * 0.5
        } else {
            500.0
        };
        let final_pen = if self.final_err > 0.02 {
            (self.final_err - 0.02) * 20000.0
        } else {
            0.0
        };
        self.iae * 2.0 + overshoot_pen + settle_pen + final_pen
    }
}

impl<H: PidHardware, const N: usize> PidController<H, N> {
    /// Auto-calibrate the controller for the current hardware.
    ///
    /// This process has three phases:
    /// 1. **Noise floor estimate**: Measures the idle jitter to set an adaptive deadband.
    /// 2. **Step response identification**: Measures the terminal velocity and mechanical time constant.
    /// 3. **Bandwidth search**: Iteratively tests a grid of candidate closed-loop bandwidths
    ///    by running real step tests on the hardware and scoring the results.
    pub async fn calibrate(&mut self) {
        self.hardware.on_calibrate_progress("Homing...");
        self.hardware.home().await;

        // Phase 0: Measure measurement noise to adapt deadband
        let noise_frac = self.estimate_noise_floor().await;
        let deadband = (2.0 * noise_frac).clamp(0.002, 0.008);
        self.set_deadband(deadband);

        // Phase 1: Identify Mechanical Plant parameters
        let (k_a, tau_samples) = match self.identify_plant().await {
            Some(res) => res,
            None => {
                self.hardware
                    .on_calibrate_progress("ID: Motor didn't move!");
                return;
            }
        };

        // Phase 2: Sweep and test candidate bandwidths
        self.hardware
            .on_calibrate_progress("Searching bandwidth...");

        let best_omega = self.search_bandwidth(k_a, tau_samples, deadband).await;

        // Apply winning gains
        self.apply_gains(best_omega, k_a, tau_samples);
        self.set_deadband(deadband);

        let mut msg = heapless::String::<64>::new();
        let _ = write!(msg, "Calib Done: w={:.3}", best_omega);
        self.hardware.on_calibrate_progress(msg.as_str());

        msg.clear();
        let _ = write!(
            msg,
            "  Kp={:.2} Kd={:.2} Ki={:.3}",
            self.kp, self.kd, self.ki
        );
        self.hardware.on_calibrate_progress(msg.as_str());
    }

    /// Measures the spread of ADC readings while the motor is fully disengaged.
    /// Returns the noise amplitude as a fraction of the total fader range.
    async fn estimate_noise_floor(&mut self) -> f32 {
        self.hardware.on_calibrate_progress("Noise floor...");

        let (min_pos, max_pos) = self.hardware.measurement_range();
        let range = (max_pos - min_pos).abs();

        let mut noise_min = f32::INFINITY;
        let mut noise_max = f32::NEG_INFINITY;

        for _ in 0..NOISE_SAMPLES {
            self.hardware.write_output(0.0).await;
            let m = self.hardware.read_measurement().await;
            if m < noise_min {
                noise_min = m;
            }
            if m > noise_max {
                noise_max = m;
            }
        }

        let noise_amp = ((noise_max - noise_min) * 0.5).max(0.0);
        noise_amp / range.max(f32::EPSILON)
    }

    /// Identifies the mechanical plant of the fader (Acceleration constant and Time constant).
    ///
    /// It drives the motor with a fixed, strong open-loop signal and observes the velocity curve.
    /// Returns `(k_a, tau_samples)` on success, or `None` if the motor fails to move.
    async fn identify_plant(&mut self) -> Option<(f32, f32)> {
        self.hardware.on_calibrate_progress("Step response ID...");
        self.drive_to_start_position().await;

        let (min_gain, max_gain) = self.hardware.gain_range();
        let force_above_min = 0.8 * (max_gain - min_gain).max(0.1);
        let test_force = min_gain + force_above_min;

        let mut pos_history = [0.0_f32; ID_DRIVE_SAMPLES];
        for pos in &mut pos_history {
            self.hardware.write_output(test_force).await;
            *pos = self.hardware.read_measurement().await;
        }
        self.hardware.write_output(0.0).await;

        // Process history to get velocities (smoothed via central difference)
        let mut vels = [0.0_f32; ID_DRIVE_SAMPLES - 2];
        let mut max_v = 0.0_f32;
        for (i, vel) in vels.iter_mut().enumerate() {
            let v = (pos_history[i + 2] - pos_history[i]).abs() / 2.0;
            *vel = v;
            if v > max_v {
                max_v = v;
            }
        }

        if max_v < 1e-4 {
            return None;
        }

        let mut tau_samples = 2.0_f32;
        let v_target = TAU_VELOCITY_FRACTION * max_v;
        for (i, &vel) in vels.iter().enumerate() {
            if vel >= v_target {
                tau_samples = (i as f32).max(1.0);
                break;
            }
        }

        let k_v = max_v / force_above_min;
        let k_a = (k_v / tau_samples).max(1e-6);

        let mut msg = heapless::String::<64>::new();
        let _ = write!(msg, "ID: t={:.1} kv={:.4} ka={:.5}", tau_samples, k_v, k_a);
        self.hardware.on_calibrate_progress(msg.as_str());

        // Coast
        for _ in 0..ID_COAST_SAMPLES {
            self.hardware.read_measurement().await;
        }

        Some((k_a, tau_samples))
    }

    /// Performs a two-pass grid search (coarse then fine) to find the optimal control bandwidth.
    ///
    /// It systematically applies candidate gains, runs physical bidirectional step tests, and
    /// scores the results to determine the fastest bandwidth that does not overshoot or drift.
    async fn search_bandwidth(&mut self, k_a: f32, tau_samples: f32, deadband: f32) -> f32 {
        let mut best_omega = OMEGA_MIN;
        let mut best_score = f32::MAX;

        // Coarse grid
        for i in 0..COARSE_GRID_STEPS {
            let frac = (i as f32) / ((COARSE_GRID_STEPS - 1) as f32);
            let omega = OMEGA_MIN * powf(OMEGA_MAX / OMEGA_MIN, frac);

            let score = self.try_bandwidth(omega, k_a, tau_samples, deadband).await;

            let mut msg = heapless::String::<64>::new();
            let _ = write!(msg, "Trial w={:.3}", omega);
            self.hardware.on_calibrate_progress(msg.as_str());

            msg.clear();
            let _ = write!(msg, "  IAE={:.1} OS={:.3}", score.iae, score.overshoot);
            self.hardware.on_calibrate_progress(msg.as_str());

            msg.clear();
            let _ = write!(msg, "  Err={:.4}", score.final_err);
            self.hardware.on_calibrate_progress(msg.as_str());

            let s = score.scalar();
            if s < best_score {
                best_score = s;
                best_omega = omega;
            }
        }

        // Refinement grid around the coarse winner
        let lo = (best_omega / 1.2).max(OMEGA_MIN);
        let hi = (best_omega * 1.2).min(OMEGA_MAX);

        if hi / lo > 1.05 {
            for i in 0..FINE_GRID_STEPS {
                let frac = (i as f32) / ((FINE_GRID_STEPS - 1) as f32);
                let omega = lo * powf(hi / lo, frac);

                let score = self.try_bandwidth(omega, k_a, tau_samples, deadband).await;

                let mut msg = heapless::String::<64>::new();
                let _ = write!(msg, "Fine w={:.3}", omega);
                self.hardware.on_calibrate_progress(msg.as_str());

                msg.clear();
                let _ = write!(msg, "  IAE={:.1} OS={:.3}", score.iae, score.overshoot);
                self.hardware.on_calibrate_progress(msg.as_str());

                let s = score.scalar();
                if s < best_score {
                    best_score = s;
                    best_omega = omega;
                }
            }
        }

        best_omega
    }

    /// Move to a specific target position using a simple open-loop drive to avoid
    /// bouncing when testing unstable candidate gains.
    async fn drive_to_specific_position(&mut self, target_pos: f32) {
        let (_min_pos, _max_pos) = self.hardware.measurement_range();
        let (min_gain, max_gain) = self.hardware.gain_range();

        let mut m = self.hardware.read_measurement().await;
        let go_positive = m < target_pos;
        let sign = if go_positive { 1.0 } else { -1.0 };

        let force = sign * (min_gain + 0.4 * (max_gain - min_gain));

        for _ in 0..1000u32 {
            self.hardware.write_output(force).await;
            m = self.hardware.read_measurement().await;
            if go_positive && m >= target_pos {
                break;
            }
            if !go_positive && m <= target_pos {
                break;
            }
        }
        self.hardware.write_output(0.0).await;
        for _ in 0..30u32 {
            self.hardware.read_measurement().await;
        }
    }

    /// Move to a "start of test" position (~10% of travel) safely.
    async fn drive_to_start_position(&mut self) {
        let (min_pos, max_pos) = self.hardware.measurement_range();
        let range = max_pos - min_pos;
        let target_pos = min_pos + 0.1 * range;
        self.drive_to_specific_position(target_pos).await;
    }

    /// Set PD+I gains for a target bandwidth omega_n (rad/sample) given identified plant.
    fn apply_gains(&mut self, omega_n: f32, k_a: f32, tau: f32) {
        let ka = k_a.max(1e-6);
        let mut kp = (omega_n * omega_n) / ka;

        let plant_damping = 1.0 / tau.max(1.0);
        let desired_damping = 2.0 * TARGET_ZETA * omega_n;
        let mut kd = (desired_damping - plant_damping).max(0.0) / ka;

        // Prevent aggressive Kd from causing jitter
        if kd > MAX_KD {
            kd = MAX_KD;
            // proportionally clamp Kp to maintain damping ratio
            let achieved_damping = kd * ka + plant_damping;
            let max_omega = achieved_damping / (2.0 * TARGET_ZETA);
            kp = kp.min(max_omega * max_omega / ka);
        }

        let ki = 0.2 * kp * omega_n;

        // Wide clamps that prevent extreme runaway
        self.kp = kp.clamp(KP_CLAMP.0, KP_CLAMP.1);
        self.kd = kd;
        self.ki = ki.clamp(KI_CLAMP.0, KI_CLAMP.1);
        self.kf = 0.0;

        let alpha = (2.0 * omega_n).clamp(0.05, 0.4);
        self.set_filter_alpha(alpha);
    }

    /// Helper to execute a single step-test trial and return its score.
    async fn run_trial_step(&mut self, goal: f32, range: f32) -> TrialScore {
        self.target = goal;
        self.reset_history();

        let start = self.hardware.read_measurement().await;
        let step_size = (goal - start).abs().max(f32::EPSILON);
        let sign = (goal - start).signum();

        let mut iae = 0.0_f32;
        let mut peak_beyond = 0.0_f32;
        let mut settle_sample: Option<u32> = None;
        let mut last_m = start;

        for i in 0..TRIAL_HORIZON {
            let m = self.step().await;
            last_m = m;
            let err = goal - m;
            iae += err.abs();
            let beyond = sign * (m - goal);
            if beyond > peak_beyond {
                peak_beyond = beyond;
            }
            if err.abs() <= SETTLE_BAND {
                if settle_sample.is_none() {
                    settle_sample = Some(i);
                }
            } else {
                settle_sample = None;
            }
        }

        self.hardware.write_output(0.0).await;

        // Observe coasting after shutoff to heavily penalize unstable final states
        for _ in 0..COAST_OBSERVATION_SAMPLES {
            let m = self.hardware.read_measurement().await;
            let beyond = sign * (m - goal);
            if beyond > peak_beyond {
                peak_beyond = beyond;
            }
            last_m = m;
        }

        let final_err = (goal - last_m).abs() / range.max(f32::EPSILON);

        TrialScore {
            iae,
            overshoot: peak_beyond / step_size,
            settled: settle_sample.is_some(),
            settle_sample: settle_sample.unwrap_or(u32::MAX),
            final_err,
        }
    }

    /// Run a bidirectional scripted step test and return its combined score.
    async fn try_bandwidth(
        &mut self,
        omega_n: f32,
        k_a: f32,
        tau: f32,
        deadband: f32,
    ) -> TrialScore {
        self.apply_gains(omega_n, k_a, tau);
        self.set_deadband(deadband);

        let (min_pos, max_pos) = self.hardware.measurement_range();
        let range = (max_pos - min_pos).abs();

        let low_pos = min_pos + 0.2 * range;
        let high_pos = min_pos + 0.8 * range;

        let current = self.hardware.read_measurement().await;
        // Determine nearest end to prep
        let (prep_pos, test1_pos, test2_pos) =
            if (current - low_pos).abs() < (current - high_pos).abs() {
                (low_pos, high_pos, low_pos)
            } else {
                (high_pos, low_pos, high_pos)
            };

        // Prep
        self.drive_to_specific_position(prep_pos).await;

        let score1 = self.run_trial_step(test1_pos, range).await;
        let score2 = self.run_trial_step(test2_pos, range).await;

        TrialScore {
            iae: score1.iae + score2.iae,
            overshoot: score1.overshoot.max(score2.overshoot),
            settled: score1.settled && score2.settled,
            settle_sample: score1.settle_sample.max(score2.settle_sample),
            final_err: score1.final_err.max(score2.final_err),
        }
    }
}

/// Computes x^exp using a rough no_std approximation.
fn powf(base: f32, exp: f32) -> f32 {
    if base <= 0.0 {
        return 0.0;
    }
    expf(exp * lnf(base))
}

/// Computes ln(x) using a rough no_std approximation.
fn lnf(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    let m_bits = (bits & 0x007f_ffff) | 0x3f80_0000;
    let m = f32::from_bits(m_bits);
    let y = (m - 1.0) / (m + 1.0);
    let y2 = y * y;
    let ln_m = 2.0 * (y + y2 * y / 3.0 + y2 * y2 * y / 5.0 + y2 * y2 * y2 * y / 7.0);
    (e as f32) * core::f32::consts::LN_2 + ln_m
}

/// Computes exp(x) using a rough no_std approximation.
fn expf(x: f32) -> f32 {
    let k = (x / core::f32::consts::LN_2 + if x >= 0.0 { 0.5 } else { -0.5 }) as i32;
    let r = x - (k as f32) * core::f32::consts::LN_2;
    let er = 1.0
        + r * (1.0
            + r * (1.0 / 2.0
                + r * (1.0 / 6.0
                    + r * (1.0 / 24.0
                        + r * (1.0 / 120.0 + r * (1.0 / 720.0 + r * (1.0 / 5040.0)))))));
    let biased = (k + 127).clamp(0, 255) as u32;
    let scale = f32::from_bits(biased << 23);
    er * scale
}
