use core::sync::atomic::Ordering;
use common::OutgoingMessage;
use crate::fader::FaderState;
use crate::system::{SystemState, SystemMode};
use crate::comms::CommsBus;

#[embassy_executor::task]
pub async fn task(
    fader: &'static FaderState,
    system: &'static SystemState,
    comms: &'static CommsBus,
) {
    loop {
        fader.sig_vol_changed.wait().await;
        if system.get_mode() == SystemMode::Standby {
            let vol = fader.volume_ppm.load(Ordering::Relaxed) as f32 / 1_000_000.0;
            let _ = comms.chan_outgoing.send(OutgoingMessage::Volume { value: vol }).await;
        }
    }
}
