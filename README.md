# DHD — Dial Hifi Device

<p align="center">
  <img src="doc/img/final_product_open.jpg" alt="DHD — a physical motorized fader for computer volume control" width="500">
</p>

<p align="center">
  <strong>A physical motorized fader for seamless computer volume control.</strong>
</p>

<p align="center">
  <a href="https://offworld-nexus-dhd.surge.sh">Documentation</a> ·
  <a href="https://offworld-nexus-dhd.surge.sh/assembly/">Assembly Guide</a> ·
  <a href="https://github.com/Xowap/dhd/tree/develop/hardware/stl">STL Files</a>
</p>

---

Move the fader by hand to adjust volume. Change volume from your computer and
watch the fader glide to match. Bidirectional, seamless, satisfying.

The DHD uses a **Bourns PSL60 motorized fader** with a PID-controlled DC motor,
driven by a **Raspberry Pi Pico 2** and a **DRV8833 motor driver**. The host
daemon on Linux integrates with PulseAudio/PipeWire for system volume control.

The enclosure is designed as a **GEN2 drawer** — it slides directly into the
[GEN2 modular system](https://www.jerrari3d.com/gen2-modular-system) with no
custom fixation needed.

## Features

- **Bidirectional control** — physical fader ↔ software volume, always in sync
- **Auto-calibration** — PID controller tunes itself to your hardware
- **GEN2 compatible** — standard drawer form factor, mounts in seconds
- **Plug and play** — USB auto-discovery, no configuration needed
- **Persistent** — calibration saved to flash, survives reboots

## Quick Start

See the full [documentation](https://offworld-nexus-dhd.surge.sh) for detailed
assembly and installation instructions.

```bash
# Clone
git clone https://github.com/Xowap/dhd.git
cd dhd

# Flash firmware (hold BOOTSEL while plugging in Pico)
rustup target add thumbv8m.main-none-eabihf
cd firmware && cargo run --release && cd ..

# Install host daemon
cd host && cargo run --release -- install && cd ..
```

## Project Structure

```
dhd/
├── common/      # Shared library — protocol + PID controller (no_std)
├── firmware/    # Embassy async firmware for RP2350
├── host/        # Linux host daemon (Tokio + PulseAudio + systray)
├── simulator/   # TUI PID simulator for tuning without hardware
├── hardware/
│   ├── kicad/   # PCB schematics
│   └── stl/     # 3D printable parts
└── doc/         # Documentation source (Zensical)
```

## Documentation

The full documentation is published at
**[offworld-nexus-dhd.surge.sh](https://offworld-nexus-dhd.surge.sh)** and
covers:

- Assembly instructions (BOM, PCB, 3D printing, step-by-step)
- Host software reference (installation, usage, protocol)
- Internals (PID control, motor driver, wiring)

## License

This project is open source. Hardware (KiCad + STL), firmware, and host software
are all available in this repository.
