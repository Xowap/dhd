# DHD Wiring Guide

This document describes the physical connections between the Raspberry Pi Pico 2 (RP2350) and the hardware components.

## Potentiometer

The system uses a standard analog potentiometer to control the volume. The Raspberry Pi Pico 2's ADC (Analog-to-Digital Converter) samples the voltage from the wiper.

| Component | Pico Pin | Function |
|-----------|----------|----------|
| Pot Terminal 1 | **3.3V (OUT)** | Positive reference |
| Pot Terminal 2 (Wiper) | **GP26 (Pin 31)** | Analog signal (ADC0) |
| Pot Terminal 3 | **GND** | Ground reference |

### Notes:
- GP26 is configured as ADC Channel 0.
- The firmware includes software-based median filtering and hysteresis to ensure stable readings even with noisy potentiometers.

## Status LED

The device uses the onboard LED (or an external one connected to the same pin) to indicate power status.

| Component | Pico Pin | Function |
|-----------|----------|----------|
| Onboard LED | **GP25** | Power Indicator |

## USB Connection

The device connects to the host computer via the micro-USB or USB-C port (depending on your specific board variant). This connection provides:
1. Power to the board and potentiometer.
2. Bi-directional JSON communication for volume updates and logs.
3. Firmware flashing (via `picotool` or BOOTSEL mode).
