use crate::fader::FaderState;
use core::sync::atomic::Ordering;
use micromath::F32Ext;

/// Background task responsible for converting raw ADC values into logical volume.
///
/// This task listens for stable, filtered raw ADC values
/// (`sig_stable_raw_changed`) and recalibrates them using the dynamically
/// updated physical boundaries.
///
/// To prevent ADC noise from causing rapid downstream mode switching, the
/// interpolated volume is strictly rounded to the nearest 1% before being
/// stored in `volume_ppm`. Finally, it emits `sig_vol_changed` to alert the
/// rest of the system (like the orchestrator or the reporter task) that a new,
/// processed logical volume is available.
///
/// If scale inversion is enabled, the normalized value is flipped (1.0 - norm)
/// so that hardware 100% reads as volume 0% and vice-versa.
#[embassy_executor::task]
pub async fn fader_task(state: &'static FaderState) {
    let mut last_raw = 0u16;
    loop {
        match embassy_futures::select::select(
            state.sig_stable_raw_changed.wait(),
            state.sig_range_updated.wait(),
        )
        .await
        {
            embassy_futures::select::Either::First(raw) => {
                last_raw = raw;
            }
            embassy_futures::select::Either::Second(_) => { /* Range updated in state */ }
        }

        let (norm, inverted) = {
            let cal = state.calibration.lock().await;
            let n = cal.physical.boundaries.interpolate(last_raw);
            (n, cal.inverted)
        };

        let norm = if inverted { 1.0 - norm } else { norm };

        // Round to nearest percent at the output of the filter
        let rounded_ppm = ((norm * 100.0).round() * 10_000.0) as u32;
        state.volume_ppm.store(rounded_ppm, Ordering::Relaxed);

        // Signal for ANY update from the filter
        state.sig_vol_changed.signal(());
    }
}
