pub use common::SystemMode;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

pub mod logger;
pub mod storage;

/// State and signals for device-level coordination.
pub struct SystemState {
    mode: AtomicU32,
    /// Fired when the host or system requests a calibration run.
    pub sig_start_calib: Signal<CriticalSectionRawMutex, bool>,
    /// Fired when the initial handshake with the host is completed.
    pub sig_handshake_done: Signal<CriticalSectionRawMutex, ()>,
    /// Fired when the system mode has changed.
    pub sig_mode_changed: Signal<CriticalSectionRawMutex, ()>,
}

impl SystemState {
    pub const fn new() -> Self {
        Self {
            mode: AtomicU32::new(SystemMode::Init as u32),
            sig_start_calib: Signal::new(),
            sig_handshake_done: Signal::new(),
            sig_mode_changed: Signal::new(),
        }
    }

    pub fn set_mode(&self, mode: SystemMode) {
        self.mode.store(mode as u32, Ordering::Relaxed);
        self.sig_mode_changed.signal(());
    }

    pub fn get_mode(&self) -> SystemMode {
        match self.mode.load(Ordering::Relaxed) {
            1 => SystemMode::Calibration,
            2 => SystemMode::Standby,
            3 => SystemMode::Failsafe,
            4 => SystemMode::PhysicallyDriven,
            5 => SystemMode::LogicallyDriven,
            _ => SystemMode::Init,
        }
    }
}

pub static STATE: SystemState = SystemState::new();
