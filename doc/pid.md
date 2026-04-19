# Motorized Fader PID Control

This document describes the design, physical principles, and implementation of the PID control system used for the motorized faders in the DHD project.

## 1. Problem Overview

A motorized fader is a mechanical system consisting of a knob, a belt/string, and a DC motor. From a control perspective, it is a **second-order mechanical plant** with significant non-linearities caused by friction.

The goal is to drive the fader to a target position ($0.0$ to $1.0$) as quickly as possible with:
- Zero steady-state error.
- Minimal overshoot (faders should feel "weighted" and "solid").
- No high-frequency oscillation or "chatter" (limit cycles).
- Robustness against measurement noise from low-resolution ADCs.

## 2. Physical Principles

### 2.1 The Plant Model
We model the fader using Newton's Second Law ($F = ma$). The net force on the fader is the sum of the motor force and the opposing friction forces:

$$m \cdot \frac{d^2x}{dt^2} = F_{motor} - F_{friction}$$

Where:
- $x$ is the position.
- $m$ is the effective mass of the knob and belt assembly.

### 2.2 Friction Model
Friction in these systems is complex and consists of three primary components:

1.  **Stiction (Static Friction):** The minimum force required to start movement from rest. In our code, this is handled by `min_gain`.
2.  **Kinetic Friction (Coulomb):** A constant force that opposes motion once the fader is moving, independent of velocity.
3.  **Viscous Damping:** A force proportional to velocity ($F_{visc} = -b \cdot v$).

### 2.3 System Gain ($k_a$)
We define the system gain $k_a$ as the acceleration produced per unit of "force above stiction":
$$a = k_a \cdot (F_{motor} - F_{stiction})$$
In physical terms, $k_a = 1/m$. Identifying this value accurately is the key to stable tuning.

## 3. Controller Architecture

The controller is implemented in `common/src/pid/controller.rs` as a modified PD+I loop.

### 3.1 Derivative-on-Measurement (DoM)
Standard PID applies the derivative term to the error ($D = K_d \cdot \frac{d(error)}{dt}$). This causes "setpoint kick" (a massive spike in output whenever the target changes).

Instead, we use **Derivative-on-Measurement**:
$$D = -K_d \cdot v_{estimated}$$
We estimate velocity $v$ using a single-pole Exponential Moving Average (EMA) filter on the position differences. This effectively rejects high-frequency ADC noise while providing the damping necessary to stop the fader's momentum.

### 3.2 Friction-Aware Output Mapping
Because the motor won't move until the force exceeds stiction, a naive PID output results in "dead" regions. We solve this by mapping the PID effort $u$ into the usable motor range:
$$Output = \text{sign}(u) \cdot (min\_gain + |u|)$$
This ensures that the smallest correction from the PID loop results in physical movement, eliminating the "stiction deadzone" without creating a discontinuity.

### 3.3 Hysteretic Deadbanding
To prevent the motor from "hunting" (vibrating) around the target due to ADC noise, we use two thresholds:
1.  **Engage Band:** The motor stays off until the error exceeds 1.5x the noise floor.
2.  **Disengage Band:** Once moving, the motor stays active until the error is within 1x the noise floor AND velocity is near zero.

### 3.4 Integral Anti-Windup
The integral term is only allowed to accumulate if the output is not already saturated. This prevents "wind-up," where the controller "remembers" a large error from a long move and overshoots the target while trying to "repay" that error.

## 4. Auto-Calibration Algorithm

The calibration logic in `common/src/pid/calibration.rs` is hardware-agnostic and discovers the optimal parameters for any fader.

### Phase 1: System Identification (ID)
We apply a constant force and observe the resulting trajectory:
1.  **Dead Time ($L$):** We measure the number of samples before the first detectable movement occurs.
2.  **Acceleration Fit ($k_a$):** We record the position at two specific time probes ($T_1, T_2$). Since $x(t) \approx 0.5 \cdot a \cdot t^2$ during the early phase, we can solve for the plant's acceleration constant $k_a$.

### Phase 2: Bandwidth Search ($\omega_n$)
We parameterize the entire PID loop using a single variable: **Closed-Loop Bandwidth** ($\omega_n$).
- For each candidate $\omega_n$, we analytically calculate $K_p$, $K_i$, and $K_d$ using the **Symmetric Optimum** method, targeting a slightly overdamped response ($\zeta = 1.1$).
- We run a "trial" step test on the hardware for each candidate.
- We score each trial using a scalar cost function:
  $$Score = (\text{IAE} \cdot 3) + \text{OvershootPenalty} + \text{SettlingTime}$$
  *IAE = Integral of Absolute Error.*

The algorithm performs a coarse grid search followed by a fine refinement to pick the fastest $\omega_n$ that doesn't cause excessive overshoot.

## 5. References

- [PID Controller (Wikipedia)](https://en.wikipedia.org/wiki/Proportional%E2%80%93integral%23derivative_controller)
- [Integral of Absolute Error (IAE)](https://en.wikipedia.org/wiki/Control_theory#Performance_measurement)
- [Symmetric Optimum Tuning](https://en.wikipedia.org/wiki/Ziegler%E2%80%93Nichols_method#Other_methods) (Note: We use a modified version for 2nd-order plants).
- [Anti-windup](https://en.wikipedia.org/wiki/Integral_windup)
