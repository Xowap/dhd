use common::OutgoingMessage;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel as MsgChannel;

pub mod usb;
pub mod reporter;

pub struct CommsBus {
    pub chan_outgoing: MsgChannel<CriticalSectionRawMutex, OutgoingMessage, 256>,
}

impl CommsBus {
    pub const fn new() -> Self {
        Self {
            chan_outgoing: MsgChannel::new(),
        }
    }
}

pub static BUS: CommsBus = CommsBus::new();
