use embassy_rp::usb::Driver;
use embassy_rp::peripherals::USB;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender};
use embassy_usb::Builder;
use core::sync::atomic::Ordering;
use common::{IncomingMessage, OutgoingMessage};
use crate::fader::FaderState;
use crate::system::SystemState;
use crate::comms::CommsBus;

#[embassy_executor::task]
pub async fn task(
    builder: Builder<'static, Driver<'static, USB>>, 
    class: CdcAcmClass<'static, Driver<'static, USB>>,
    fader: &'static FaderState,
    system: &'static SystemState,
    comms: &'static CommsBus,
) {
    let mut usb = builder.build();
    let (sender, receiver) = class.split();
    
    let usb_fut = usb.run();
    let rx_fut = run_rx_loop(receiver, fader, system, comms);
    let tx_fut = run_tx_loop(sender, comms);

    embassy_futures::select::select3(usb_fut, rx_fut, tx_fut).await;
}

async fn run_rx_loop(
    mut receiver: Receiver<'static, Driver<'static, USB>>,
    fader: &'static FaderState,
    system: &'static SystemState,
    comms: &'static CommsBus,
) {
    let mut in_buf = [0u8; 256];
    loop {
        receiver.wait_connection().await;
        loop {
            match receiver.read_packet(&mut in_buf).await {
                Ok(size) if size > 0 => {
                    handle_incoming_packet(&in_buf[..size], fader, system, comms);
                }
                Ok(_) => {}
                Err(_) => break, // Connection lost
            }
        }
    }
}

async fn run_tx_loop(
    mut sender: Sender<'static, Driver<'static, USB>>,
    comms: &'static CommsBus,
) {
    let mut out_buf = [0u8; 258];
    loop {
        sender.wait_connection().await;
        loop {
            let msg = comms.chan_outgoing.receive().await;
            if let Ok(size) = serde_json_core::to_slice(&msg, &mut out_buf[..256]) {
                out_buf[size] = b'\n';
                if sender.write_packet(&out_buf[..size+1]).await.is_err() {
                    break;
                }
            }
            
            // Proactively drain up to 10 more messages to improve throughput
            // but don't stay here forever to avoid starving the executor if many tasks are logging
            for _ in 0..10 {
                if let Ok(queued_msg) = comms.chan_outgoing.try_receive() {
                    if let Ok(size) = serde_json_core::to_slice(&queued_msg, &mut out_buf[..256]) {
                        out_buf[size] = b'\n';
                        if sender.write_packet(&out_buf[..size+1]).await.is_err() {
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
        }
    }
}

fn handle_incoming_packet(
    data: &[u8],
    fader: &'static FaderState,
    system: &'static SystemState,
    comms: &'static CommsBus,
) {
    // Basic newline trimming
    let data = if !data.is_empty() && data[data.len()-1] == b'\n' { &data[..data.len()-1] } else { data };
    let data = if !data.is_empty() && data[data.len()-1] == b'\r' { &data[..data.len()-1] } else { data };
    if data.is_empty() { return; }

    match serde_json_core::from_slice::<IncomingMessage>(data) {
        Ok((IncomingMessage::Ping { timestamp }, _)) => {
            let _ = comms.chan_outgoing.try_send(OutgoingMessage::Pong { timestamp });
        }
        Ok((IncomingMessage::Handshake { .. }, _)) => {
            let mut res = heapless::String::<32>::new();
            let _ = core::fmt::write(&mut res, format_args!("Tek'ma'te Bra'tac"));
            let _ = comms.chan_outgoing.try_send(OutgoingMessage::Handshake { message: res });
            system.sig_handshake_done.signal(());
        }
        Ok((IncomingMessage::StartCalibration, _)) => {
            system.sig_start_calib.signal(());
        }
        Ok((IncomingMessage::UpdateCalibration { bottom, top }, _)) => {
            fader.bottom.store(bottom as u32, Ordering::Relaxed);
            fader.top.store(top as u32, Ordering::Relaxed);
            fader.sig_range_updated.signal(());
        }
        _ => {}
    }
}
