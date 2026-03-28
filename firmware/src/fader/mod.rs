use core::sync::atomic::AtomicU32;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

pub mod interface;
pub mod service;
pub mod calibration;
pub mod reader;

/// State and signals related to the physical fader hardware.
pub struct FaderState {
    /// Scaling factor for speed, which mostly serves to orient the speed
    /// control in a way that makes sense with the potentiometer's axis
    pub speed_scale: f32,
    
    pub last_raw_adc: AtomicU32,
    pub volume_ppm: AtomicU32,
    pub bottom: AtomicU32,
    pub top: AtomicU32,
    
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
            speed_scale: 1.0,
            last_raw_adc: AtomicU32::new(0),
            volume_ppm: AtomicU32::new(0),
            bottom: AtomicU32::new(82),
            top: AtomicU32::new(4013),
            sig_raw_changed: Signal::new(),
            sig_stable_raw_changed: Signal::new(),
            sig_vol_changed: Signal::new(),
            sig_range_updated: Signal::new(),
        }
    }
}

pub static STATE: FaderState = FaderState::new();
