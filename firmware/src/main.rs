//! DHD (Dial Hifi Device) Firmware
//!
//! Domain-driven architecture for Raspberry Pi Pico 2.

#![no_std]
#![no_main]

mod fader;
mod comms;
mod system;
pub mod utils;

use embassy_executor::Spawner;
use embassy_rp::adc::{Adc, Config as AdcConfig, Channel};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::usb::Driver;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::InterruptHandler as UsbInterruptHandler;
use embassy_rp::adc::InterruptHandler as AdcInterruptHandler;
use embassy_time::Instant;
use embassy_usb::Builder;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State as CdcState};
use embassy_usb::Config as UsbConfig;
use static_cell::StaticCell;
use {panic_halt as _};

use crate::system::{STATE as SYSTEM, SystemMode, logger::LOGGER};
use crate::fader::{STATE as FADER, interface::FaderInterface, service::FaderService, calibration::CalibrationService, reader::reader_task};
use crate::comms::{BUS as COMMS, usb::task as usb_task, reporter::task as reporter_task};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => UsbInterruptHandler<USB>;
    ADC_IRQ_FIFO => AdcInterruptHandler;
});

#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"DHD - Dial Hifi Device"),
    embassy_rp::binary_info::rp_program_description!(c"Physical media controller with haptic volume feedback"),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);

    // USB Driver Setup
    let driver = Driver::new(p.USB, Irqs);
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CTRL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static STATE: StaticCell<CdcState> = StaticCell::new();

    let mut config = UsbConfig::new(0x2e8a, 0x000a);
    config.manufacturer = Some("Offworld Nexus");
    config.product = Some("Dial Hifi Device");
    config.serial_number = Some("DHD-DEV");
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    let mut builder = Builder::new(driver, config, CONFIG_DESC.init([0; 256]), BOS_DESC.init([0; 256]), &mut [], CTRL_BUF.init([0; 64]));
    let class = CdcAcmClass::new(&mut builder, STATE.init(CdcState::new()), 64);

    // Hardware setup
    let pwm_c = PwmConfig::default();
    let pwm = Pwm::new_output_ab(p.PWM_SLICE7, p.PIN_14, p.PIN_15, pwm_c);
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());
    let channel = Channel::new_pin(p.PIN_26, embassy_rp::gpio::Pull::None);

    // Domain infrastructure assembly
    let fader_hw = FaderInterface::new(pwm, &FADER);
    
    static CAL_BUF: StaticCell<[(Instant, u16); 300]> = StaticCell::new();
    let mut cal_svc = CalibrationService::new(
        fader_hw, 
        &FADER,
        &COMMS,
        CAL_BUF.init([(Instant::now(), 0); 300]), 
        21
    );
    let mut fader_svc = FaderService::new(&FADER);

    // Spawn Domain tasks
    spawner.spawn(usb_task(builder, class, &FADER, &SYSTEM, &COMMS).unwrap());
    spawner.spawn(reader_task(adc, channel, &FADER, 21).unwrap());
    spawner.spawn(reporter_task(&FADER, &SYSTEM, &COMMS).unwrap());

    // Orchestrator State Machine
    loop {
        match SYSTEM.get_mode() {
            SystemMode::Init => {
                SYSTEM.sig_handshake_done.wait().await;
                while COMMS.chan_outgoing.try_receive().is_ok() {}
                SYSTEM.set_mode(SystemMode::Calibration);
            }
            SystemMode::Calibration => {
                cal_svc.run_calibration().await;
                SYSTEM.set_mode(SystemMode::Standby);
            }
            SystemMode::Standby => {
                embassy_futures::select::select(
                    fader_svc.run(), 
                    SYSTEM.sig_start_calib.wait()
                ).await;
                SYSTEM.set_mode(SystemMode::Calibration);
            }
        }
    }
}
