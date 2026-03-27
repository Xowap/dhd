use core::sync::atomic::Ordering;
use embassy_time::{Duration, Instant, Timer};
use crate::fader::{FaderState, interface::FaderInterface};
use crate::comms::CommsBus;

pub struct SpeedEstimator {
    samples: [(f32, f32); 8],
    count: usize,
    min_pwm: f32,
}

impl SpeedEstimator {
    pub const fn new() -> Self {
        Self {
            samples: [(0.0, 0.0); 8],
            count: 0,
            min_pwm: 1.0,
        }
    }

    pub fn update(&mut self, samples: &[(f32, f32)], min_pwm: f32) {
        let len = samples.len().min(8);
        for i in 0..len { self.samples[i] = samples[i]; }
        self.count = len;
        self.min_pwm = min_pwm;
    }
}

pub struct CalibrationService {
    fader: FaderInterface,
    fader_state: &'static FaderState,
    comms: &'static CommsBus,
    samples_buf: &'static mut [(Instant, u16)],
    hysteresis: u16,
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
        Timer::after_millis(20).await;
        
        let (rb, rt) = self.run_rough_sweep().await;
        let (min_pwm, speeds) = self.run_speed_sweep(rb, rt).await;
        self.run_fine_crawl(min_pwm, speeds).await;
        
        log::info!("Calibration complete.");
    }

    async fn run_rough_sweep(&mut self) -> (u16, u16) {
        log::info!("Rough sweep...");
        let r1 = self.fader.drive_until_stall(0.4).await;
        let r2 = self.fader.drive_until_stall(-0.4).await;
        let (rb, rt) = if r1 < r2 { (r1, r2) } else { (r2, r1) };
        log::info!("Rough range: {} - {}", rb, rt);
        Timer::after_millis(20).await;
        (rb, rt)
    }

    async fn run_speed_sweep(&mut self, rb: u16, rt: u16) -> (f32, [(f32, f32); 8]) {
        let pwms = [0.15, 0.2, 0.25, 0.3, 0.4, 0.6, 0.8, 1.0];
        let mut speeds = [(0.0, 0.0); 8];
        let mut min_pwm = 1.0;
        let mut found_min = false;

        for (i, &p) in pwms.iter().enumerate() {
            log::info!("Testing PWM {}...", p);
            let s = self.measure_speed(p, rt, rb).await;
            speeds[i] = (p, s);
            if s > 0.005 {
                log::info!("speed({}) = {:.3} u/s", p, s);
                if !found_min {
                    min_pwm = if i == 0 { p * 0.8 } else { pwms[i-1] + (p - pwms[i-1]) * 0.5 };
                    found_min = true;
                }
            } else { log::info!("speed({}) = insufficient", p); }
            
            // Yield to allow other tasks to run and USB to breathe
            Timer::after_millis(20).await;
        }
        (min_pwm, speeds)
    }

    async fn run_fine_crawl(&mut self, min_pwm: f32, speeds: [(f32, f32); 8]) {
        let crawl = (min_pwm * 1.3).clamp(0.12, 0.4);
        log::info!("Fine crawl at PWM {:.3}...", crawl);
        let e1 = self.fader.drive_until_stall(crawl).await;
        let e2 = self.fader.drive_until_stall(-crawl).await;

        let (b, t) = if e1 < e2 { (e1, e2) } else { (e2, e1) };
        let cb = b.saturating_add(self.hysteresis);
        let ct = t.saturating_sub(self.hysteresis);
        
        log::info!("Final Range: {} - {}", cb, ct);
        self.fader_state.bottom.store(cb as u32, Ordering::Relaxed);
        self.fader_state.top.store(ct as u32, Ordering::Relaxed);
        self.fader_state.sig_range_updated.signal(());
        
        let mut est = SpeedEstimator::new();
        est.update(&speeds, min_pwm);
        self.comms.sig_speed_est.signal(est);
    }

    async fn measure_speed(&mut self, pwm: f32, top: u16, bottom: u16) -> f32 {
        let range = (top as i32 - bottom as i32).abs() as f32;
        if range < 1.0 { return 0.0; }
        let current = self.fader.get_raw_pos();
        let (target_start, direction) = if (current as i32 - bottom as i32).abs() < (current as i32 - top as i32).abs() {
            (bottom, 1.0f32)
        } else {
            (top, -1.0f32)
        };

        // Only move to start if we are not already close enough
        if (current as i32 - target_start as i32).abs() > 100 {
            let _ = self.fader.drive_until_stall(-direction * 0.4).await;
        }

        let mut count = 0;
        self.fader.set_motor_speed(direction * pwm);
        let start = Instant::now();
        let mut last_pos = self.fader.get_raw_pos();
        let mut timer = Instant::now();

        loop {
            let val = self.fader_state.sig_raw_changed.wait().await;
            if count < self.samples_buf.len() {
                self.samples_buf[count] = (Instant::now(), val);
                count += 1;
            }
            if (val as i32 - last_pos as i32).abs() > 5 {
                timer = Instant::now();
            } else if timer.elapsed() > Duration::from_millis(300) {
                if (val as i32 - target_start as i32).abs() > (range * 0.1) as i32 { break; }
            }
            last_pos = val;
            if start.elapsed() > Duration::from_secs(6) { break; }
        }
        self.fader.set_motor_speed(0.0);
        if count < 30 { return 0.0; }

        self.calculate_speed_from_samples(count, range)
    }

    fn calculate_speed_from_samples(&self, count: usize, range: f32) -> f32 {
        let start_val = self.samples_buf[0].1;
        let mut m_start = 0;
        for i in 0..count {
            if (self.samples_buf[i].1 as i32 - start_val as i32).abs() > self.hysteresis as i32 * 2 {
                m_start = i; break;
            }
        }
        let final_val = self.samples_buf[count-1].1;
        let mut m_end = count - 1;
        for i in (m_start..count).rev() {
            if (self.samples_buf[i].1 as i32 - final_val as i32).abs() > self.hysteresis as i32 * 2 {
                m_end = i; break;
            }
        }

        if m_end < m_start + 30 {
            let total_delta = (final_val as i32 - start_val as i32).abs();
            if total_delta < (range * 0.2) as i32 { return 0.0; }
            let d_pos = total_delta as f32 / range;
            let d_time = self.samples_buf[count-1].0.duration_since(self.samples_buf[0].0).as_micros() as f32 / 1_000_000.0;
            return d_pos / d_time;
        }

        let d_pos = (self.samples_buf[m_end].1 as i32 - self.samples_buf[m_start].1 as i32).abs() as f32 / range;
        let d_time = self.samples_buf[m_end].0.duration_since(self.samples_buf[m_start].0).as_micros() as f32 / 1_000_000.0;
        if d_time < 0.001 { 0.0 } else { d_pos / d_time }
    }
}
