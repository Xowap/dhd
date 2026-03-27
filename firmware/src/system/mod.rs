use core::sync::atomic::{AtomicU32, Ordering};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

pub mod logger;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SystemMode {
    Init = 0,
    Calibration = 1,
    Standby = 2,
}

/// State and signals for device-level coordination.
pub struct SystemState {
    mode: AtomicU32,
    /// Fired when the host or system requests a calibration run.
    pub sig_start_calib: Signal<CriticalSectionRawMutex, ()>,
}

impl SystemState {
    pub const fn new() -> Self {
        Self {
            mode: AtomicU32::new(SystemMode::Init as u32),
            sig_start_calib: Signal::new(),
        }
    }

    pub fn set_mode(&self, mode: SystemMode) {
        self.mode.store(mode as u32, Ordering::Relaxed);
    }

    pub fn get_mode(&self) -> SystemMode {
        match self.mode.load(Ordering::Relaxed) {
            1 => SystemMode::Calibration,
            2 => SystemMode::Standby,
            _ => SystemMode::Init,
        }
    }
}

pub static STATE: SystemState = SystemState::new();
