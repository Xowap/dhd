use crate::comms::BUS as COMMS;
use common::OutgoingMessage;
use heapless::String;

pub struct JsonLogger;

impl log::Log for JsonLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let mut level_str = String::<16>::new();
            let _ = core::fmt::write(&mut level_str, format_args!("{}", record.level()));
            let mut msg_str = String::<384>::new();
            let _ = core::fmt::write(&mut msg_str, format_args!("{}", record.args()));
            let _ = COMMS.chan_outgoing.try_send(OutgoingMessage::Log {
                level: level_str,
                message: msg_str,
            });
        }
    }
    fn flush(&self) {}
}

pub static LOGGER: JsonLogger = JsonLogger;
