//! DHD Simulator
//!
//! A Terminal User Interface (TUI) application simulating the mechanical plant
//! and PID controller of the Dial Hifi Device.

use common::pid::{PidController, PidHardware};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use rand::Rng;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    symbols,
    widgets::{Block, Borders, Chart, Dataset, Paragraph},
};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Internal state of the simulated mechanical plant.
struct SimulationState {
    position: f32,
    velocity: f32,
    acceleration: f32,
    mass: f32,
    friction: f32,
    last_update: Instant,
    history: Vec<(f64, f64)>,
    target_history: Vec<(f64, f64)>,
    start_time: Instant,

    // PID Parameters
    kp: f32,
    ki: f32,
    kd: f32,
    kf: f32,
    target: f32,

    // Noise
    noise_level: f32,
    running: bool,
    pid_enabled: bool,
    calibrating: bool,
    move_until_target: Option<f32>,
    calibration_msg: String,
}

impl SimulationState {
    /// Create a new simulation state with default physical parameters.
    fn new() -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            acceleration: 0.0,
            mass: 1.0,
            friction: 0.5,
            last_update: Instant::now(),
            history: Vec::new(),
            target_history: Vec::new(),
            start_time: Instant::now(),
            kp: 2.0,
            ki: 0.1,
            kd: 0.5,
            kf: 0.0,
            target: 0.5,
            noise_level: 0.01,
            running: true,
            pid_enabled: true,
            calibrating: false,
            move_until_target: None,
            calibration_msg: "Idle".to_string(),
        }
    }

    /// Advance the physics by one time step based on the applied force.
    fn update(&mut self, force: f32) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f32();
        self.last_update = now;

        let total_force = force - (self.velocity * self.friction);
        self.acceleration = total_force / self.mass;

        self.velocity += self.acceleration * dt;
        self.position += self.velocity * dt;

        if self.position < 0.0 {
            self.position = 0.0;
            self.velocity = 0.0;
        } else if self.position > 1.0 {
            self.position = 1.0;
            self.velocity = 0.0;
        }

        let elapsed = now.duration_since(self.start_time).as_secs_f64();
        self.history.push((elapsed, self.position as f64));
        self.target_history.push((elapsed, self.target as f64));

        if self.history.len() > 500 {
            self.history.remove(0);
            self.target_history.remove(0);
        }
    }
}

/// A hardware bridge that maps the `PidHardware` trait to the simulation state.
struct SimulatedActuator {
    state: Arc<Mutex<SimulationState>>,
}

impl PidHardware for SimulatedActuator {
    async fn read_measurement(&mut self) -> f32 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let s = self.state.lock().unwrap();
        let mut rng = rand::thread_rng();
        let noise: f32 = rng.gen_range(-s.noise_level..s.noise_level);
        s.position + noise
    }

    async fn write_output(&mut self, value: f32) {
        let mut s = self.state.lock().unwrap();
        s.update(value);
    }

    fn gain_range(&self) -> (f32, f32) {
        (1.0, 5.0)
    }

    fn measurement_range(&self) -> (f32, f32) {
        (0.0, 1.0)
    }

    fn midpoint(&self) -> f32 {
        0.5
    }

    async fn home(&mut self) {
        // Magical jump to zero
        let mut s = self.state.lock().unwrap();
        s.position = 0.0;
        s.velocity = 0.0;
        s.acceleration = 0.0;
        s.calibration_msg = "Homed!".to_string();
    }

    fn on_calibrate_progress(&mut self, msg: &str) {
        let mut s = self.state.lock().unwrap();
        s.calibration_msg = msg.to_string();
    }
}

/// Main entry point for the TUI simulator.
#[tokio::main]
async fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let state = Arc::new(Mutex::new(SimulationState::new()));

    let pid_state = state.clone();
    tokio::spawn(async move {
        let actuator = SimulatedActuator {
            state: pid_state.clone(),
        };
        let mut pid = PidController::<_, 50>::new(actuator, 2.0, 0.1, 0.5, 0.0);

        loop {
            let (kp, ki, kd, kf, target, running, pid_enabled, calibrate, move_target) = {
                let mut s = pid_state.lock().unwrap();
                let c = s.calibrating;
                if c {
                    s.calibrating = false;
                }
                let mt = s.move_until_target.take();
                (
                    s.kp,
                    s.ki,
                    s.kd,
                    s.kf,
                    s.target,
                    s.running,
                    s.pid_enabled,
                    c,
                    mt,
                )
            };

            if !running {
                break;
            }

            if let Some(t) = move_target {
                pid.set_coefficients(kp, ki, kd, kf);
                pid.run_until_target(t).await;
                let mut s = pid_state.lock().unwrap();
                s.pid_enabled = false;
                s.calibration_msg = "Target reached. PID disabled.".to_string();
            } else if calibrate {
                pid.calibrate().await;
                // Update shared state with new calibrated values
                let mut s = pid_state.lock().unwrap();
                s.kp = pid.kp;
                s.ki = pid.ki;
                s.kd = pid.kd;
                s.kf = pid.kf;
                s.target = pid.target;
                s.calibrating = false; // Ensure it's cleared
                s.pid_enabled = true;
            } else if pid_enabled {
                pid.set_coefficients(kp, ki, kd, kf);
                pid.set_target(target);
                pid.step().await;
            } else {
                // PID disabled: just wait a bit and don't apply force
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    });

    loop {
        terminal.draw(|f| ui(f, &state))?;

        if event::poll(Duration::from_millis(20))?
            && let Event::Key(key) = event::read()?
        {
            let mut s = state.lock().unwrap();
            match key.code {
                KeyCode::Char('q') => {
                    s.running = false;
                    break;
                }
                KeyCode::Char('c') => s.calibrating = true,
                KeyCode::Char('x') => {
                    let mut rng = rand::thread_rng();
                    let target = rng.gen_range(0.0..1.0);
                    s.target = target;
                    s.move_until_target = Some(target);
                    s.calibration_msg = format!("Moving to {:.2}...", target);
                }
                KeyCode::Char('p') => s.pid_enabled = !s.pid_enabled,
                KeyCode::Char('w') => s.kp += 0.1,
                KeyCode::Char('s') => s.kp -= 0.1,
                KeyCode::Char('e') => s.ki += 0.01,
                KeyCode::Char('d') => s.ki -= 0.01,
                KeyCode::Char('r') => s.kd += 0.05,
                KeyCode::Char('f') => s.kd -= 0.05,
                KeyCode::Char('t') => s.target = (s.target + 0.05).min(1.0),
                KeyCode::Char('g') => s.target = (s.target - 0.05).max(0.0),
                KeyCode::Char('y') => s.kf += 0.05,
                KeyCode::Char('h') => s.kf -= 0.05,
                KeyCode::Char(' ') => {
                    s.position = 0.0;
                    s.velocity = 0.0;
                    s.acceleration = 0.0;
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// Render the TUI layout and widgets.
fn ui(f: &mut Frame, state: &Arc<Mutex<SimulationState>>) {
    let s = state.lock().unwrap();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(6),
        ])
        .split(f.size());

    let title = Paragraph::new(format!(
        "PID [{}] | Kp:{:.2} Ki:{:.2} Kd:{:.2} Kf:{:.2} | Target:{:.2} | C:Calib X:RunOnce P:Toggle Space:Reset Q:Quit",
        if s.pid_enabled { "ON" } else { "OFF" },
        s.kp, s.ki, s.kd, s.kf, s.target
    ))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let x_bounds = [
        s.history.first().map(|h| h.0).unwrap_or(0.0),
        s.history.last().map(|h| h.0).unwrap_or(10.0),
    ];

    let datasets = vec![
        Dataset::default()
            .name("Position")
            .marker(symbols::Marker::Dot)
            .style(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan))
            .data(&s.history),
        Dataset::default()
            .name("Target")
            .marker(symbols::Marker::Braille)
            .style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow))
            .data(&s.target_history),
    ];

    let chart = Chart::new(datasets)
        .block(Block::default().title("Response").borders(Borders::ALL))
        .x_axis(
            ratatui::widgets::Axis::default()
                .title("Time (s)")
                .style(ratatui::style::Style::default().fg(ratatui::style::Color::Gray))
                .bounds(x_bounds),
        )
        .y_axis(
            ratatui::widgets::Axis::default()
                .title("Value")
                .style(ratatui::style::Style::default().fg(ratatui::style::Color::Gray))
                .bounds([0.0, 1.0]),
        );
    f.render_widget(chart, chunks[1]);

    let stats = Paragraph::new(format!(
        "Pos: {:.4} | Vel: {:.4} | Acc: {:.4} | Error: {:.4}\nStatus: {}",
        s.position,
        s.velocity,
        s.acceleration,
        s.target - s.position,
        s.calibration_msg
    ))
    .block(Block::default().title("Stats").borders(Borders::ALL));
    f.render_widget(stats, chunks[2]);
}
