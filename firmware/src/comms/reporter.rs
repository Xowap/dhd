use crate::comms::CommsBus;
use crate::fader::FaderState;
use crate::system::{SystemMode, SystemState};
use common::OutgoingMessage;
use core::sync::atomic::Ordering;
use embassy_futures::select::{Either, select};

/// Background task responsible for dispatching state updates to the host.
///
/// This task independently observes changes in the logical volume (`sig_vol_changed`)
/// and the system operation mode (`sig_mode_changed`). By decoupling reporting from
/// the actual data processing (like the ADC filter or USB parser), the system ensures
/// the host always receives the latest state without blocking internal control loops.
///
/// Volume updates are only reported during `Standby` or `PhysicallyDriven` modes,
/// preventing the device from endlessly echoing back the host's own commands during
/// logical driving or calibration.
#[embassy_executor::task]
pub async fn task(
    fader: &'static FaderState,
    system: &'static SystemState,
    comms: &'static CommsBus,
) {
    loop {
        match select(fader.sig_vol_changed.wait(), system.sig_mode_changed.wait()).await {
            Either::First(_) => {
                let mode = system.get_mode();
                if mode == SystemMode::Standby || mode == SystemMode::PhysicallyDriven {
                    let vol = fader.volume_ppm.load(Ordering::Relaxed) as f32 / 1_000_000.0;
                    let _ = comms
                        .chan_outgoing
                        .send(OutgoingMessage::Volume { value: vol })
                        .await;
                }
            }
            Either::Second(_) => {
                let _ = comms
                    .chan_outgoing
                    .send(OutgoingMessage::Mode {
                        mode: system.get_mode(),
                    })
                    .await;
            }
        }
    }
}
