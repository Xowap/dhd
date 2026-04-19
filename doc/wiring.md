# DHD Wiring Guide

This document describes the physical connections between the Raspberry Pi Pico 2 (RP2350) and the hardware components.

## Potentiometer

The system uses a standard analog potentiometer to control the volume. The Raspberry Pi Pico 2's ADC (Analog-to-Digital Converter) samples the voltage from the wiper.

| Component              | Pico Pin          | Function             |
| ---------------------- | ----------------- | -------------------- |
| Pot Terminal 1         | **3.3V (OUT)**    | Positive reference   |
| Pot Terminal 2 (Wiper) | **GP26 (Pin 31)** | Analog signal (ADC0) |
| Pot Terminal 3         | **GND**           | Ground reference     |

### Notes:

- GP26 is configured as ADC Channel 0.
- The firmware includes software-based median filtering and hysteresis to ensure stable readings even with noisy potentiometers.

## Status LED

The device uses the onboard LED (or an external one connected to the same pin) to indicate power status.

| Component   | Pico Pin | Function        |
| ----------- | -------- | --------------- |
| Onboard LED | **GP25** | Power Indicator |

## USB Connection

The device connects to the host computer via the micro-USB or USB-C port (depending on your specific board variant). This connection provides:

1. Power to the board and potentiometer.
2. Bi-directional JSON communication for volume updates and logs.
3. Firmware flashing (via `picotool` or BOOTSEL mode).

## Motor Driver (DRV8833)

The motor driver controls the motorized potentiometer to provide physical feedback and remote adjustments.

| DRV8833 Pin             | Connects To            | Purpose                                                                         |
| :---------------------- | :--------------------- | :------------------------------------------------------------------------------ |
| **VM** (or VCC/Motor)   | **Pico Pin 40 (VBUS)** | Feeds raw 5V from USB to the motor.                                             |
| **GND**                 | **Pico Pin 38 (GND)**  | Completes the power circuit.                                                    |
| **IN1**                 | **GP14 (Pin 19)**      | Motor logic (Forward).                                                          |
| **IN2**                 | **GP15 (Pin 20)**      | Motor logic (Backward).                                                         |
| **OUT1**                | Fader Motor Pin A      | Power out to the physical motor.                                                |
| **OUT2**                | Fader Motor Pin B      | Power out to the physical motor.                                                |
| **EEP** (or SLP/STBY)\* | **3.3V OUT (Pin 36)**  | If your board has this pin, it must be connected to 3.3V to "wake up" the chip. |

### Notes:

- **VM** is connected to **VBUS** (5V) rather than 3.3V to provide sufficient torque for the motor.
- **GP14** and **GP15** are used as PWM outputs to control motor speed and direction.
- Ensure common ground between the Pico and the motor driver.
