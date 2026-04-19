use core::sync::atomic::AtomicU32;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;

pub mod calibration;
pub mod interface;
pub mod reader;
pub mod service;

use crate::fader::calibration::CalibrationResult;

/// State and signals related to the physical fader hardware.
pub struct FaderState {
    pub last_raw_adc: AtomicU32,
    pub volume_ppm: AtomicU32,
    pub calibration: Mutex<CriticalSectionRawMutex, CalibrationResult>,

    /// Fired when a new ADC sample is ready.
    pub sig_raw_changed: Signal<CriticalSectionRawMutex, u16>,
    /// Fired when a stable (filtered) ADC value is ready.
    pub sig_stable_raw_changed: Signal<CriticalSectionRawMutex, u16>,
    /// Fired when the normalized volume has been recalculated.
    pub sig_vol_changed: Signal<CriticalSectionRawMutex, ()>,
    /// Fired when calibration boundaries have changed.
    pub sig_range_updated: Signal<CriticalSectionRawMutex, ()>,
}

impl FaderState {
    pub const fn new() -> Self {
        Self {
            last_raw_adc: AtomicU32::new(0),
            volume_ppm: AtomicU32::new(0),
            calibration: Mutex::new(CalibrationResult {
                boundaries: calibration::Boundaries {
                    min: 82,
                    max: 4013,
                    speed_scale: 1.0,
                },
                lowest_speed: 0.1,
            }),
            sig_raw_changed: Signal::new(),
            sig_stable_raw_changed: Signal::new(),
            sig_vol_changed: Signal::new(),
            sig_range_updated: Signal::new(),
        }
    }
}

pub static STATE: FaderState = FaderState::new();
