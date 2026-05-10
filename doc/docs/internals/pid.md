# PID Control System

This document describes the design and implementation of the PID control system
used for the motorized fader.

## Problem Overview

A motorized fader is a **second-order mechanical plant** with significant
non-linearities caused by friction. The control goal is to drive the fader to a
target position (0.0 to 1.0) with:

- Zero steady-state error
- Minimal overshoot (solid, weighted feel)
- No high-frequency oscillation or chatter
- Robustness against ADC noise

## Physical Model

The fader is modeled using Newton's Second Law:

$$m \cdot \frac{d^2x}{dt^2} = F_{motor} - F_{friction}$$

### Friction Components

1. **Stiction** — minimum force to start movement (handled by `min_gain`)
2. **Kinetic friction** — constant opposing force during motion
3. **Viscous damping** — force proportional to velocity ($F = -b \cdot v$)

### System Gain

The calibration identifies two key parameters:

- **$k_v$** — terminal velocity per unit force above stiction
- **$\tau$** — mechanical time constant (time to reach 63% of terminal velocity)
- **$k_a = k_v / \tau$** — the acceleration constant

## Controller Architecture

Implemented in `common/src/pid/controller.rs` as a modified PD+I loop.

### Derivative-on-Measurement

Instead of differentiating the error (which causes setpoint kick), we use:

$$D = -K_d \cdot v_{estimated}$$

Velocity is estimated by applying a single-pole EMA filter to position, then
taking successive differences of the filtered signal. This rejects high-frequency
ADC noise while providing the damping needed to arrest momentum.

### Friction-Aware Output Mapping

The PID effort $u$ is mapped into the usable motor range:

$$Output = \text{sign}(u) \cdot \min(min\_gain + |u|,\ max\_gain)$$

When effort is near zero, the output defaults to $\text{sign}(error) \cdot min\_gain$
to prevent stalling in the stiction deadzone.

### Hysteretic Deadbanding

Two thresholds prevent motor hunting around the target:

1. **Engage band** (1.5× noise floor) — motor stays off until error exceeds this
2. **Disengage band** (1× noise floor + velocity ≈ 0) — motor stays active until
   both conditions are met

On re-engagement, the integral term is reset to zero to prevent accumulated
wind-up from causing a kick.

### Integral Anti-Windup

The integral only accumulates when the output is not saturated **in the same
direction as the error**. This allows the integral to decrease (error sign
flipped) even while output is at the rail, preventing overshoot from stored
error.

## Auto-Calibration

Implemented in `common/src/pid/calibration.rs`. Fully hardware-agnostic.

### Phase 1 — System Identification

A constant force is applied and the velocity response is observed:

1. Record position history over a step response
2. Compute velocities via central differences
3. Find **terminal velocity** ($v_{max}$)
4. Find **time constant** ($\tau$) — time to reach 63% of $v_{max}$
5. Compute $k_v = v_{max} / F_{applied}$ and $k_a = k_v / \tau$

### Phase 2 — Bandwidth Search

The entire PID loop is parameterized by a single variable: **closed-loop
bandwidth** ($\omega_n$).

For each candidate $\omega_n$, gains are computed via pole placement:

- $K_p = \omega_n^2 / k_a$
- $K_d = (2 \cdot \zeta_{target} \cdot \omega_n - 1/\tau) / k_a$
  (clamped ≥ 0)
- $K_i = 0.2 \cdot K_p \cdot \omega_n$
- Target damping: $\zeta = 1.4$ (significantly overdamped for stability)

A bidirectional step test scores each candidate:

$$Score = IAE \times 2 + OvershootPenalty + SettlePenalty + FinalErrorPenalty$$

Where:

- **Overshoot penalty:** $(overshoot - 0.02) \times 10000$ if > 2%
- **Settle penalty:** $settle\_sample \times 0.5$ if settled, else 500
- **Final error penalty:** $(final\_err - 0.02) \times 20000$ if > 2%

The algorithm runs a 6-step coarse grid followed by a 3-step fine refinement
around the best candidate.

## References

- [PID Controller (Wikipedia)](https://en.wikipedia.org/wiki/Proportional%E2%80%93integral%E2%80%93derivative_controller)
- [Integral Windup](https://en.wikipedia.org/wiki/Integral_windup)
- [Ziegler-Nichols & related methods](https://en.wikipedia.org/wiki/Ziegler%E2%80%93Nichols_method)
