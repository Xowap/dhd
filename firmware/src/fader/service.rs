use crate::fader::FaderState;
use core::sync::atomic::Ordering;

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
                self.state.sig_range_updated.wait(),
            )
            .await
            {
                embassy_futures::select::Either::First(raw) => {
                    last_raw = raw;
                }
                embassy_futures::select::Either::Second(_) => { /* Range updated in state */ }
            }

            let norm = self
                .state
                .calibration
                .lock()
                .await
                .boundaries
                .interpolate(last_raw);

            self.state
                .volume_ppm
                .store((norm * 1_000_000.0) as u32, Ordering::Relaxed);
            self.state.sig_vol_changed.signal(());
        }
    }
}
