use crate::fader::FaderState;
use core::sync::atomic::Ordering;
use embassy_rp::adc::{Adc, Async, Channel};
use embassy_time::{Duration, Ticker};

/// Background task responsible for sampling the raw ADC at a high frequency.
///
/// This task operates at 500Hz (2ms interval) to read the physical potentiometer.
/// It immediately broadcasts the raw ADC value (`sig_raw_changed`) for components 
/// requiring zero latency (like calibration boundary detection). 
/// 
/// Simultaneously, it maintains a running Median Filter to scrub out analog noise 
/// and electrical transients. A hysteresis threshold is applied to this filtered output;
/// only meaningful, stable changes cause a `sig_stable_raw_changed` event. This effectively
/// provides the rest of the firmware with a clean, jitter-free view of the fader's position.
#[embassy_executor::task]
pub async fn reader_task(
    mut adc: Adc<'static, Async>,
    mut ch: Channel<'static>,
    state: &'static FaderState,
    hysteresis: u16,
) {
    let mut ticker = Ticker::every(Duration::from_millis(2));
    let mut filter = MedianFilter::<15>::new();
    let mut last_stable = 0u16;
    let mut initialized = false;

    loop {
        if let Ok(raw) = adc.read(&mut ch).await {
            // Unfiltered data is always updated immediately
            state.last_raw_adc.store(raw as u32, Ordering::Relaxed);
            state.sig_raw_changed.signal(raw);

            // Filtered data for stability
            if let Some(median) = filter.push(raw) {
                let changed = if !initialized {
                    last_stable = median;
                    initialized = true;
                    true
                } else {
                    (median as i32 - last_stable as i32).abs() > hysteresis as i32
                };

                if changed {
                    last_stable = median;
                    state.sig_stable_raw_changed.signal(median);
                }
            }
        }
        ticker.next().await;
    }
}

struct MedianFilter<const N: usize> {
    buffer: [u16; N],
    index: usize,
    count: usize,
}

impl<const N: usize> MedianFilter<N> {
    fn new() -> Self {
        Self {
            buffer: [0; N],
            index: 0,
            count: 0,
        }
    }

    fn push(&mut self, val: u16) -> Option<u16> {
        self.buffer[self.index % N] = val;
        self.index += 1;
        if self.count < N {
            self.count += 1;
        }

        if self.count == N {
            let mut sort_buf = self.buffer;
            sort_buf.sort_unstable();
            Some(sort_buf[N / 2])
        } else {
            None
        }
    }
}
