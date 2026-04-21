//! DHD (Dial Hifi Device) Firmware
//!
//! Domain-driven architecture for Raspberry Pi Pico 2.

#![no_std]
#![no_main]

mod comms;
mod fader;
mod system;
pub mod utils;

use core::sync::atomic::Ordering;
use embassy_executor::Spawner;
use embassy_futures::select::{Either3, select3};
use embassy_rp::adc::InterruptHandler as AdcInterruptHandler;
use embassy_rp::adc::{Adc, Channel, Config as AdcConfig};
use embassy_rp::bind_interrupts;
use embassy_rp::dma::InterruptHandler as DmaInterruptHandler;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::{DMA_CH0, FLASH, USB};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::usb::Driver;
use embassy_rp::usb::InterruptHandler as UsbInterruptHandler;
use embassy_time::{Duration, Timer};
use embassy_usb::Builder;
use embassy_usb::Config as UsbConfig;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State as CdcState};
use panic_halt as _;
use static_cell::StaticCell;

use crate::comms::{BUS as COMMS, reporter::task as reporter_task, usb::task as usb_task};
use crate::fader::{
    STATE as FADER, calibration::CalibrationService, interface::FaderInterface,
    reader::reader_task, service::fader_task,
};
use crate::system::storage::{FLASH_SIZE, load_calibration, store_calibration};
use crate::system::{STATE as SYSTEM, SystemMode, logger::LOGGER};
use common::pid::{PidController, PidHardware};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => UsbInterruptHandler<USB>;
    ADC_IRQ_FIFO => AdcInterruptHandler;
    DMA_IRQ_0 => DmaInterruptHandler<DMA_CH0>;
});

#[unsafe(link_section = ".bi_entries")]
#[used]
/// Embedded binary information for the picotool utility.
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"DHD - Dial Hifi Device"),
    embassy_rp::binary_info::rp_program_description!(
        c"Physical media controller with haptic volume feedback"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

/// Entry point for the firmware.
///
/// Initializes the RP2350 hardware, USB communication, ADC, and PWM.
/// Spawns the background tasks for USB handling, ADC reading, fader
/// interpolation, and host reporting.
/// Runs the main orchestrator state machine that coordinates system modes.
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);

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

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        &mut [],
        CTRL_BUF.init([0; 64]),
    );
    let class = CdcAcmClass::new(&mut builder, STATE.init(CdcState::new()), 64);

    // Hardware setup
    let mut pwm_c = PwmConfig::default();
    pwm_c.top = 10000;
    let pwm = Pwm::new_output_ab(p.PWM_SLICE7, p.PIN_14, p.PIN_15, pwm_c);
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());
    let channel = Channel::new_pin(p.PIN_26, embassy_rp::gpio::Pull::None);

    // Flash setup for persistent storage
    let mut flash = Flash::<_, Async, FLASH_SIZE>::new(p.FLASH, p.DMA_CH0, Irqs);

    // Domain infrastructure assembly
    let fader_hw = FaderInterface::new(pwm, &FADER);
    let mut cal_svc = CalibrationService::new(fader_hw);

    if let Some(cal) = load_calibration(&mut flash).await {
        log::info!("Loaded calibration from flash");
        *FADER.calibration.lock().await = cal;
    }

    // Spawn Domain tasks
    spawner.spawn(usb_task(builder, class, &FADER, &SYSTEM, &COMMS).unwrap());
    spawner.spawn(reader_task(adc, channel, &FADER, 21).unwrap());
    spawner.spawn(fader_task(&FADER).unwrap());
    spawner.spawn(reporter_task(&FADER, &SYSTEM, &COMMS).unwrap());

    // Orchestrator State Machine
    loop {
        match SYSTEM.get_mode() {
            SystemMode::Init => handle_init().await,
            SystemMode::Calibration => handle_calibration(&mut cal_svc, &mut flash, true).await,
            SystemMode::Standby => handle_standby(&mut cal_svc, &mut flash).await,
            SystemMode::LogicallyDriven => handle_logically_driven(&mut cal_svc).await,
            SystemMode::PhysicallyDriven => handle_physically_driven(&mut cal_svc).await,
            SystemMode::Failsafe => handle_failsafe().await,
        }
    }
}

/// Initializes the device connection.
///
/// Waits for the initial handshake to complete with the host, ensuring the USB
/// CDC-ACM connection is fully established. It then drains any pending outgoing
/// messages before transitioning the system to the `Calibration` state to
/// determine physical boundaries.
async fn handle_init() {
    SYSTEM.sig_handshake_done.wait().await;
    while COMMS.chan_outgoing.try_receive().is_ok() {}
    SYSTEM.set_mode(SystemMode::Standby);
}

/// Executes the dual-stage calibration routine.
///
/// First, it runs a physical calibration to detect the hard mechanical stops
/// (0% and 100%) and saves these boundaries to ensure safe motor operation.
/// Next, it performs an autotune sequence to calculate the PID coefficients
/// (Kp, Ki, Kd) necessary for precise logical driving.
/// Finally, it moves the fader to the initial logical volume requested by the
/// host before transitioning into the `Standby` state.
async fn handle_calibration(
    cal_svc: &mut CalibrationService,
    flash: &mut Flash<'_, FLASH, Async, FLASH_SIZE>,
    force: bool,
) {
    SYSTEM.set_mode(SystemMode::Calibration);

    let mut needs_run = force;
    if !force {
        let cal = FADER.calibration.lock().await;
        if cal.physical.boundaries.max <= cal.physical.boundaries.min {
            needs_run = true;
        }
    }

    if needs_run {
        log::info!("Starting physical calibration...");
        let physical = cal_svc.run_physical_calibration().await;
        FADER.calibration.lock().await.physical = physical;

        log::info!("Starting PID calibration...");
        let pid_cal = cal_svc.run_pid_calibration().await;
        let cal_to_save = {
            let mut cal = FADER.calibration.lock().await;
            cal.pid = pid_cal;
            *cal
        };
        store_calibration(flash, &cal_to_save).await;
    } else {
        log::info!("Calibration already exists, skipping re-calibration");
    }

    log::info!("All calibrations complete.");

    let pid_cal = FADER.calibration.lock().await.pid;
    let target: f32 = FADER.target_volume_ppm.load(Ordering::Relaxed) as f32 / 1_000_000.0;
    log::debug!("Moving to final target: {:.2}", target);

    let hw = cal_svc.take_fader();
    let mut pid = PidController::<_, 50>::new(hw, pid_cal.kp, pid_cal.ki, pid_cal.kd, 0.0);
    pid.alpha = pid_cal.alpha;
    pid.deadband_frac = pid_cal.deadband;

    pid.run_until_target(target).await;

    pid.hardware.write_output(0.0).await;
    cal_svc.return_fader(pid.hardware);

    FADER.sig_target_vol_changed.reset();
    FADER.sig_vol_changed.reset();
    SYSTEM.set_mode(SystemMode::Standby);
}

/// Handles the idle state of the device.
///
/// In standby, the motor is completely disengaged. This function awaits external
/// triggers to change the system mode:
/// - A host-requested calibration -> `Calibration`
/// - A volume update from the host -> `LogicallyDriven`
/// - Physical movement detected from the user -> `PhysicallyDriven`
async fn handle_standby(
    cal_svc: &mut CalibrationService,
    flash: &mut Flash<'_, FLASH, Async, FLASH_SIZE>,
) {
    match select3(
        SYSTEM.sig_start_calib.wait(),
        FADER.sig_target_vol_changed.wait(),
        FADER.sig_vol_changed.wait(),
    )
    .await
    {
        Either3::First(force) => {
            handle_calibration(cal_svc, flash, force).await;
        }
        Either3::Second(_) => {
            SYSTEM.set_mode(SystemMode::LogicallyDriven);
        }
        Either3::Third(_) => {
            SYSTEM.set_mode(SystemMode::PhysicallyDriven);
        }
    }
}

/// Handles the logic for `LogicallyDriven` mode.
///
/// In this mode, the system acts as an output device for the host. The host
/// sends volume updates which the PID controller must track. The `fader_task`
/// continuously runs the filter and updates `sig_vol_changed`. The PID
/// controller runs until it reaches the target volume and the plant settles
/// mechanically (signaled by `!pid.was_driving`).
///
/// If the host sends a new target while the fader is still moving to the
/// previous one, the inner loop is interrupted and restarts with the new
/// target. If the PID takes longer than 1 second to settle on a single target,
/// it times out and aborts to prevent burning out the motor.
async fn handle_logically_driven(cal_svc: &mut CalibrationService) {
    log::info!("Mode: LogicallyDriven");
    let cal_data = FADER.calibration.lock().await;
    let mut pid = PidController::<_, 50>::new(
        cal_svc.take_fader(),
        cal_data.pid.kp,
        cal_data.pid.ki,
        cal_data.pid.kd,
        0.0,
    );
    pid.alpha = cal_data.pid.alpha;
    pid.deadband_frac = cal_data.pid.deadband;
    drop(cal_data);

    loop {
        FADER.sig_target_vol_changed.reset();
        let target: f32 = FADER.target_volume_ppm.load(Ordering::Relaxed) as f32 / 1_000_000.0;

        match select3(
            pid.run_until_target(target),
            FADER.sig_target_vol_changed.wait(),
            Timer::after(Duration::from_secs(1)),
        )
        .await
        {
            Either3::First(_) => break,     // Reached target
            Either3::Second(_) => continue, // New target, restart loop
            Either3::Third(_) => {
                log::warn!("LogicallyDriven timed out");
                break;
            }
        }
    }

    pid.hardware.write_output(0.0).await;
    cal_svc.return_fader(pid.hardware);
    FADER.sig_vol_changed.reset();
    SYSTEM.set_mode(SystemMode::Standby);
}

/// Handles the logic for `PhysicallyDriven` mode.
///
/// This mode is activated when the user physically manipulates the fader knob.
/// The purpose of this mode is to allow the user to freely change the volume,
/// which is then reported back to the host.
///
/// To prevent feedback resonance, the motor is completely disengaged (output
/// set to `0.0`). The system simply waits and observes `sig_vol_changed`. If
/// no volume changes occur for 1 full second, it is assumed the user has
/// stopped touching the fader, and the system transitions back to `Standby`.
async fn handle_physically_driven(cal_svc: &mut CalibrationService) {
    log::info!("Mode: PhysicallyDriven");

    // Disable motor for physical movements
    let mut hw = cal_svc.take_fader();
    hw.write_output(0.0).await;

    // Wait until no physical movement is detected for 1 second
    while let embassy_futures::select::Either::First(_) = embassy_futures::select::select(
        FADER.sig_vol_changed.wait(),
        Timer::after(Duration::from_secs(1)),
    )
    .await
    {
        // Movement detected, continue waiting
    }

    cal_svc.return_fader(hw);

    // Explicitly reset the volume changed signal to avoid immediate re-entry
    FADER.sig_vol_changed.reset();
    SYSTEM.set_mode(SystemMode::Standby);
}

/// A fallback mode entered if a critical subsystem crashes or an irrecoverable
/// error occurs.
///
/// Once in Failsafe mode, the device halts all operations and waits
/// indefinitely, requiring a hard reset or power cycle to recover.
async fn handle_failsafe() {
    log::warn!("Failsafe mode");
    loop {
        embassy_futures::yield_now().await;
    }
}
