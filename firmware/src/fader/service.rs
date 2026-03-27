use core::sync::atomic::Ordering;
use crate::fader::FaderState;

/// Service that handles normal fader operation (position to volume conversion).
pub struct FaderService {
    state: &'static FaderState,
}

impl FaderService {
    pub fn new(state: &'static FaderState) -> Self {
        Self { state }
    }

    pub async fn run(&mut self) -> ! {
        let mut last_raw = 0u16;
        loop {
            match embassy_futures::select::select(
                self.state.sig_stable_raw_changed.wait(),
                self.state.sig_range_updated.wait()
            ).await {
                embassy_futures::select::Either::First(raw) => { last_raw = raw; }
                embassy_futures::select::Either::Second(_) => { /* Range updated in state */ }
            }
            
            let b = self.state.bottom.load(Ordering::Relaxed) as u16;
            let t = self.state.top.load(Ordering::Relaxed) as u16;

            let norm = if last_raw <= b { 0.0 } 
                      else if last_raw >= t { 1.0 } 
                      else { (last_raw - b) as f32 / (t - b) as f32 };
            
            self.state.volume_ppm.store((norm * 1_000_000.0) as u32, Ordering::Relaxed);
            self.state.sig_vol_changed.signal(());
        }
    }
}
