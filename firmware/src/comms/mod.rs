use common::OutgoingMessage;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel as MsgChannel;
use embassy_sync::signal::Signal;
use crate::fader::calibration::SpeedEstimator;

pub mod usb;
pub mod reporter;

pub struct CommsBus {
    pub chan_outgoing: MsgChannel<CriticalSectionRawMutex, OutgoingMessage, 256>,
    pub sig_speed_est: Signal<CriticalSectionRawMutex, SpeedEstimator>,
}

impl CommsBus {
    pub const fn new() -> Self {
        Self {
            chan_outgoing: MsgChannel::new(),
            sig_speed_est: Signal::new(),
        }
    }
}

pub static BUS: CommsBus = CommsBus::new();
