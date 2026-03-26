# DHD - Dial Hifi Device

DHD is a physical media controller for computers. It provides tactile control over volume and media playback, featuring a high-quality potentiometer interface and a Linux-native host tool for seamless integration.

## Project Structure

This project is organized as a Cargo workspace:

- **`common/`**: Shared library containing the JSON protocol definitions used by both the firmware and the host.
- **`firmware/`**: The embedded Rust firmware for the Raspberry Pi Pico 2 (RP2350), built with the Embassy async framework.
- **`host/`**: A Linux terminal application (`dhd`) that communicates with the device, displays real-time volume updates, and monitors device health.

## Hardware Requirements

- **Microcontroller**: Raspberry Pi Pico 2 (RP2350).
- **Control**: A 10k (or similar) analog potentiometer.
- **Wiring**: See [doc/wiring.md](doc/wiring.md) for detailed connection instructions.

## Getting Started

### 1. Build and Flash the Firmware

You will need the `thumbv8m.main-none-eabihf` target installed:
```bash
rustup target add thumbv8m.main-none-eabihf
```

To build and flash the device (requires `picotool`):
```bash
cd firmware
cargo run --release
```
*Note: The firmware automatically targets the RP2350 and handles linker configurations via its internal `build.rs`.*

### 2. Run the Host Tool

The host tool runs on your Linux machine and automatically detects the DHD when it is plugged in.

From the project root:
```bash
cargo run -p dhd
```

## Features

- **Responsive Volume Control**: Uses ADC sampling with median filtering and hysteresis for a smooth, jitter-free experience.
- **Auto-Discovery**: The host tool automatically scans USB ports and reconnects to the device if it's unplugged.
- **JSON Protocol**: Bi-directional communication allowing for complex commands and telemetry.
- **Remote Logging**: Device logs are sent as JSON over USB and rendered clearly in the host terminal.
- **Health Monitoring**: Periodic ping/pong mechanism to ensure the link is active.
