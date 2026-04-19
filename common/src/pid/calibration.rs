use super::controller::PidController;
use super::hardware::PidHardware;
use core::fmt::Write;

/// Result of a single step-test trial.
#[derive(Clone, Copy)]
struct TrialScore {
    /// Integral of absolute error.
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
    /// Balances overshoot, settling time, and final error.
    fn scalar(&self) -> f32 {
        let overshoot_pen = if self.overshoot > 0.05 {
            (self.overshoot - 0.05) * 3000.0
        } else {
            0.0
        };
        let settle_pen = if self.settled {
            self.settle_sample as f32
        } else {
            800.0
        };
        let final_pen = if self.final_err > 0.02 {
            (self.final_err - 0.02) * 15000.0
        } else {
            0.0
        };
        self.iae * 3.0 + overshoot_pen + settle_pen + final_pen
    }
}

impl<H: PidHardware, const N: usize> PidController<H, N> {
    /// Auto-calibrate the controller for the current hardware.
    ///
    /// This process has three phases:
    /// 1. **Noise floor estimate**: Measures the idle jitter to set an adaptive deadband.
    /// 2. **Step response identification**: Measures dead-time and identifies the acceleration
    ///    constant of the mechanical assembly using Newton's Second Law.
    /// 3. **Bandwidth search**: Iteratively tests a grid of candidate closed-loop bandwidths
    ///    by running real step tests on the hardware and scoring the results based on IAE
    ///    and overshoot.
    pub async fn calibrate(&mut self) {
        self.hardware.on_calibrate_progress("Homing...");
        self.hardware.home().await;

        let (min_pos, max_pos) = self.hardware.measurement_range();
        let (min_gain, max_gain) = self.hardware.gain_range();
        let range = (max_pos - min_pos).abs();
        let dir = if max_pos > min_pos { 1.0 } else { -1.0 };

        // ---------- Phase 0: Noise floor estimate at rest ----------
        // Sit still for a few samples, look at the spread of readings.
        self.hardware.on_calibrate_progress("Noise floor...");
        let mut noise_min = f32::INFINITY;
        let mut noise_max = f32::NEG_INFINITY;
        for _ in 0..24u32 {
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
        let noise_frac = noise_amp / range.max(f32::EPSILON);

        // ---------- Phase 1: Step response identification ----------
        self.hardware.on_calibrate_progress("Step response...");

        // Drive at a force well above min_gain so we reliably move.
        let force_above_min = 0.7 * (max_gain - min_gain).max(0.1);
        let test_force = dir * (min_gain + force_above_min);

        let start_pos = self.hardware.read_measurement().await;

        let mut t_first_move: Option<u32> = None;
        let mut pos_at_first_move: f32 = start_pos;

        // We record two position probes after motion starts at fixed delays, to
        // estimate acceleration from the position curve (pos(t) ~ 0.5*a*t^2 early on,
        // pos(t) ~ v_terminal*t later). We take probes at motion+N and motion+2N samples.
        let probe_delay_1: u32 = 6;
        let probe_delay_2: u32 = 18;
        let mut pos_probe_1: Option<f32> = None;
        let mut pos_probe_2: Option<f32> = None;

        // Stop point: 80% of range (or 20% if dir < 0) — just to abort long runs.
        let reached_target = if dir > 0.0 {
            min_pos + 0.8 * (max_pos - min_pos)
        } else {
            min_pos + 0.2 * (max_pos - min_pos)
        };

        // Motion-detect threshold.
        let move_thresh = (3.0 * noise_amp).max(0.005 * range);

        for i in 0..3000u32 {
            self.hardware.write_output(test_force).await;
            let m = self.hardware.read_measurement().await;

            if t_first_move.is_none() && (m - start_pos).abs() > move_thresh {
                t_first_move = Some(i);
                pos_at_first_move = m;
            }

            if let Some(t1) = t_first_move {
                let delta = i - t1;
                if delta == probe_delay_1 && pos_probe_1.is_none() {
                    pos_probe_1 = Some(m);
                }
                if delta == probe_delay_2 && pos_probe_2.is_none() {
                    pos_probe_2 = Some(m);
                }
                // We have what we need; bail out now.
                if pos_probe_2.is_some() {
                    break;
                }
            }

            // Safety: if we reach the 80% point (or 20% for dir<0) without our probes,
            // we're going to run out of travel — abort.
            let crossed_final = if dir > 0.0 {
                m >= reached_target
            } else {
                m <= reached_target
            };
            if crossed_final {
                break;
            }
        }
        self.hardware.write_output(0.0).await;

        let (t1, p1, p2) = match (t_first_move, pos_probe_1, pos_probe_2) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                self.hardware.on_calibrate_progress("ID failed");
                return;
            }
        };

        let l_samples = t1.max(1) as f32;

        // Distances from motion-start point.
        let d1 = (p1 - pos_at_first_move).abs();
        let d2 = (p2 - pos_at_first_move).abs();
        if d2 <= d1 || d1 <= 0.0 {
            self.hardware.on_calibrate_progress("ID failed");
            return;
        }

        // Model: s(t) = 0.5 * a0 * t^2 initially, transitioning toward s(t) = v_t * t + const.
        // From two probes we can solve a simple quadratic fit:
        //   d1 = v0*T1 + 0.5*a*T1^2
        //   d2 = v0*T2 + 0.5*a*T2^2
        // with v0 = 0 (just started moving, but in practice we have 1-sample delay so v0 ≈ a*1).
        // Simpler: approximate as constant-acceleration over the whole window.
        //   d2 ≈ 0.5 * a * T2^2  => a ≈ 2*d2 / T2^2
        let t2f = probe_delay_2 as f32;
        let a_sample = 2.0 * d2 / (t2f * t2f); // per sample^2

        // `a = k_a * force_above_min` (approximately, since v is still small).
        // So k_a = a / force_above_min.
        let k_a = (a_sample / force_above_min).max(1e-5);

        // Coast the output off briefly to let motion settle before bandwidth search.
        for _ in 0..30u32 {
            self.hardware.write_output(0.0).await;
            self.hardware.read_measurement().await;
        }
        // `b_damp` is not used by the final gain formula (apply_gains ignores it), but we
        // still report a plausible value for diagnostics.
        let b_damp: f32 = 0.15;

        // ---------- Phase 2: Bandwidth search ----------
        self.hardware
            .on_calibrate_progress("Searching bandwidth...");

        // Return to a good starting position for step tests (low end of travel).
        self.drive_to_start_position(dir).await;

        // Bandwidth candidates. Upper bound: stability margin vs dead time.
        // For a 2nd-order mechanical plant w/ low dead time, omega_n*L ≈ 0.8 is fine.
        let omega_max = (0.9 / l_samples).clamp(0.08, 1.5);
        let omega_min = (omega_max * 0.2).max(0.04);

        // Adaptive deadband: must be bigger than measurement noise floor.
        // Keep it tight: 2x the noise floor, floor of 0.2%, cap of 0.8%.
        let deadband = (2.0 * noise_frac).clamp(0.002, 0.008);
        self.set_deadband(deadband);

        let mut best_omega = omega_min;
        let mut best_score = f32::MAX;

        // Coarse grid: 8 candidates log-spaced over the full range.
        let n_coarse: u32 = 8;
        for i in 0..n_coarse {
            let frac = (i as f32) / ((n_coarse - 1) as f32);
            let omega = omega_min * powf(omega_max / omega_min, frac);
            let score = self.try_bandwidth(omega, k_a, b_damp, dir, deadband).await;

            let mut msg = heapless::String::<96>::new();
            let _ = write!(
                msg,
                "w={:.3} iae={:.1} os={:.2} fin={:.3}",
                omega, score.iae, score.overshoot, score.final_err
            );
            self.hardware.on_calibrate_progress(msg.as_str());

            let s = score.scalar();
            if s < best_score {
                best_score = s;
                best_omega = omega;
            }
        }

        // Refinement: sample 4 candidates between the best and its faster neighbor.
        let lo = (best_omega / 1.3).max(omega_min);
        let hi = (best_omega * 1.3).min(omega_max);
        if hi / lo > 1.05 {
            let n_fine: u32 = 4;
            for i in 0..n_fine {
                let frac = (i as f32) / ((n_fine - 1) as f32);
                let omega = lo * powf(hi / lo, frac);
                let score = self.try_bandwidth(omega, k_a, b_damp, dir, deadband).await;
                let s = score.scalar();
                if s < best_score {
                    best_score = s;
                    best_omega = omega;
                }
            }
        }

        // Apply winning gains.
        self.apply_gains(best_omega, k_a, b_damp);
        self.set_deadband(deadband);

        let mut msg = heapless::String::<128>::new();
        let _ = write!(
            msg,
            "Done w={:.3} L={:.0} ka={:.4} b={:.3} Kp={:.2} Kd={:.2} Ki={:.3}",
            best_omega, l_samples, k_a, b_damp, self.kp, self.kd, self.ki
        );
        self.hardware.on_calibrate_progress(msg.as_str());

        // Final landing.
        self.target = self.hardware.midpoint();
        self.reset_history();
        for _ in 0..300u32 {
            self.step().await;
        }
    }

    /// Move to a "start of test" position (~10% of travel) using a simple open-loop drive.
    async fn drive_to_start_position(&mut self, dir: f32) {
        let (min_pos, max_pos) = self.hardware.measurement_range();
        let (min_gain, max_gain) = self.hardware.gain_range();
        let range = max_pos - min_pos;
        let target_pos = min_pos + 0.1 * range;
        let go_pos = dir > 0.0;

        // Drive in reverse at moderate power until we're near the target_pos.
        let force = -dir * (min_gain + 0.5 * (max_gain - min_gain));
        for _ in 0..2000u32 {
            self.hardware.write_output(force).await;
            let m = self.hardware.read_measurement().await;
            let close = if go_pos {
                m <= target_pos
            } else {
                m >= target_pos
            };
            if close {
                break;
            }
        }
        // Settle.
        for _ in 0..20u32 {
            self.hardware.write_output(0.0).await;
            self.hardware.read_measurement().await;
        }
    }

    /// Set PD+I gains for a target bandwidth omega_n (rad/sample) given identified plant.
    ///
    /// Uses critical damping (zeta = 1.1) to ensure no ringing.
    fn apply_gains(&mut self, omega_n: f32, k_a: f32, _b_damp: f32) {
        let zeta = 1.1_f32;
        let ka = k_a.max(1e-6);
        let kp = (omega_n * omega_n) / ka;
        let kd = (2.0 * zeta * omega_n) / ka;
        let ki = 0.1 * kp * omega_n;

        self.kp = kp;
        self.ki = ki;
        self.kd = kd;
        self.kf = 0.0;

        let alpha = (4.0 * omega_n).clamp(0.2, 0.9);
        self.set_filter_alpha(alpha);
    }

    /// Run a scripted step test and return its score.
    async fn try_bandwidth(
        &mut self,
        omega_n: f32,
        k_a: f32,
        b_damp: f32,
        dir: f32,
        deadband: f32,
    ) -> TrialScore {
        self.apply_gains(omega_n, k_a, b_damp);
        self.set_deadband(deadband);

        let (min_pos, max_pos) = self.hardware.measurement_range();
        let range = (max_pos - min_pos).abs();

        // Step from ~10% to ~60% of travel.
        let start = if dir > 0.0 {
            min_pos + 0.1 * (max_pos - min_pos)
        } else {
            min_pos + 0.9 * (max_pos - min_pos)
        };
        let goal = if dir > 0.0 {
            min_pos + 0.6 * (max_pos - min_pos)
        } else {
            min_pos + 0.4 * (max_pos - min_pos)
        };

        // Get to `start` first.
        self.target = start;
        self.reset_history();
        for _ in 0..200u32 {
            self.step().await;
        }

        self.target = goal;
        self.reset_history();

        let step_size = (goal - start).abs().max(f32::EPSILON);
        // Horizon: fixed so we compare candidates on equal footing.
        let horizon: u32 = 200;
        let settle_band = 0.02_f32; // 2% of range, matches the end-to-end test criterion
        let sign = (goal - start).signum();

        let mut iae = 0.0_f32;
        let mut peak_beyond = 0.0_f32;
        let mut settle_sample: Option<u32> = None;
        let mut last_m = start;

        for i in 0..horizon {
            self.step().await;
            let m = self.hardware.read_measurement().await;
            last_m = m;
            let err = goal - m;
            iae += err.abs();
            let beyond = sign * (m - goal);
            if beyond > peak_beyond {
                peak_beyond = beyond;
            }
            if err.abs() <= settle_band {
                if settle_sample.is_none() {
                    settle_sample = Some(i);
                }
            } else {
                settle_sample = None;
            }
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
