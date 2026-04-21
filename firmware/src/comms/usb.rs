use crate::comms::CommsBus;
use crate::fader::FaderState;
use crate::system::SystemState;
use common::{IncomingMessage, OutgoingMessage};
use core::sync::atomic::Ordering;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_usb::Builder;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender};

/// Background task managing all USB CDC-ACM communication.
///
/// This task isolates the USB device drivers, serialization/deserialization, and
/// buffer management from the core logic. It runs the USB polling future, the RX
/// listener, and the TX sender concurrently. By operating asynchronously, it ensures
/// that heavy protocol processing or dropped connections do not pause the critical
/// PID control loops running in the orchestrator.
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

async fn run_tx_loop(mut sender: Sender<'static, Driver<'static, USB>>, comms: &'static CommsBus) {
    let mut out_buf = [0u8; 512];
    loop {
        sender.wait_connection().await;
        loop {
            let msg = comms.chan_outgoing.receive().await;
            if let Ok(size) = serde_json_core::to_slice(&msg, &mut out_buf[..510]) {
                out_buf[size] = b'\n';
                let data = &out_buf[..size + 1];

                let mut error = false;
                let mut last_chunk_size = 0;
                for chunk in data.chunks(64) {
                    last_chunk_size = chunk.len();
                    if sender.write_packet(chunk).await.is_err() {
                        error = true;
                        break;
                    }
                }
                if !error && last_chunk_size == 64 && sender.write_packet(&[]).await.is_err() {
                    // Send a Zero-Length Packet (ZLP) to flush the 64-byte packet
                    error = true;
                }
                if error {
                    break;
                }
            }

            // Proactively drain up to 20 more messages to improve throughput
            for _ in 0..20 {
                if let Ok(queued_msg) = comms.chan_outgoing.try_receive() {
                    if let Ok(size) = serde_json_core::to_slice(&queued_msg, &mut out_buf[..510]) {
                        out_buf[size] = b'\n';
                        let data = &out_buf[..size + 1];
                        let mut error = false;
                        let mut last_chunk_size = 0;
                        for chunk in data.chunks(64) {
                            last_chunk_size = chunk.len();
                            if sender.write_packet(chunk).await.is_err() {
                                error = true;
                                break;
                            }
                        }
                        if !error
                            && last_chunk_size == 64
                            && sender.write_packet(&[]).await.is_err()
                        {
                            error = true;
                        }
                        if error {
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
    let data = if !data.is_empty() && data[data.len() - 1] == b'\n' {
        &data[..data.len() - 1]
    } else {
        data
    };
    let data = if !data.is_empty() && data[data.len() - 1] == b'\r' {
        &data[..data.len() - 1]
    } else {
        data
    };
    if data.is_empty() {
        return;
    }

    match serde_json_core::from_slice::<IncomingMessage>(data) {
        Ok((IncomingMessage::Ping { timestamp }, _)) => {
            let _ = comms
                .chan_outgoing
                .try_send(OutgoingMessage::Pong { timestamp });
        }
        Ok((IncomingMessage::Handshake { .. }, _)) => {
            let mut res = heapless::String::<32>::new();
            let _ = core::fmt::write(&mut res, format_args!("Tek'ma'te Bra'tac"));
            let _ = comms
                .chan_outgoing
                .try_send(OutgoingMessage::Handshake { message: res });
            system.sig_handshake_done.signal(());
        }
        Ok((IncomingMessage::StartCalibration { volume }, _)) => {
            fader
                .target_volume_ppm
                .store((volume * 1_000_000.0) as u32, Ordering::Relaxed);
            system.sig_start_calib.signal(());
        }
        Ok((IncomingMessage::UpdateCalibration { bottom, top }, _)) => {
            if let Ok(mut cal) = fader.calibration.try_lock() {
                cal.physical.boundaries.min = bottom;
                cal.physical.boundaries.max = top;
            }
            fader.sig_range_updated.signal(());
        }
        Ok((IncomingMessage::SetVolume { value }, _)) => {
            fader
                .target_volume_ppm
                .store((value * 1_000_000.0) as u32, Ordering::Relaxed);
            fader.sig_target_vol_changed.signal(());
        }
        _ => {}
    }
}
