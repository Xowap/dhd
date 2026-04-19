/// Hardware abstraction for the PID controller.
/// Implement this trait to connect the PID controller to your sensors and actuators.
///
/// We use `async fn` in the trait directly rather than desugaring to `impl Future`.
/// The controller only consumes the trait in a single async task (embassy executor on
/// RP2040, or a `current_thread` tokio runtime in tests), so we never need the auto-
/// traits (`Send`, `Sync`) that the lint warns about.
#[allow(async_fn_in_trait)]
pub trait PidHardware {
    /// Await the next measurement from the sensor.
    /// This method is responsible for controlling the sampling rate of the PID loop.
    async fn read_measurement(&mut self) -> f32;

    /// Apply the calculated control output (gain) to the actuator.
    async fn write_output(&mut self, value: f32);

    /// Get the allowed range for the output magnitude.
    /// Returns (min_magnitude, max_magnitude).
    /// The controller will ensure that any non-zero output has a magnitude
    /// within this range, preserving the sign.
    fn gain_range(&self) -> (f32, f32);

    /// Get the allowed range for the measurement.
    /// Returns (min_measurement, max_measurement).
    fn measurement_range(&self) -> (f32, f32);

    /// Get the midpoint value for the actuator.
    fn midpoint(&self) -> f32;

    /// Return the system to its well-known "home" state (e.g. position 0).
    async fn home(&mut self);

    /// Optional callback to report calibration progress to the user/host.
    fn on_calibrate_progress(&mut self, _msg: &str) {}
}
