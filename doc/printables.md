# DHD — Dial Hifi Device

A physical motorized fader that provides seamless bidirectional volume control for your computer.

Move the fader by hand to adjust volume. Change volume from your computer and watch the fader glide to match. Bidirectional, seamless, satisfying.

## What Is This?

The DHD uses a Bourns PSL60 motorized fader with a PID-controlled DC motor, driven by a Raspberry Pi Pico 2 and a DRV8833 motor driver. The host daemon on Linux integrates with PulseAudio/PipeWire for system volume control.

The enclosure is designed as a **GEN2 drawer** — it slides directly into the [GEN2 modular system](https://www.jerrari3d.com/gen2-modular-system) with no custom fixation needed.

**Drawer specs:** 185 depth, 2W×1H

## Features

- Bidirectional control — physical fader and software volume always stay in sync
- Auto-calibration — the PID controller tunes itself to your hardware
- GEN2 compatible — standard drawer form factor
- Plug and play — USB auto-discovery, no configuration

## What You Need

- Bourns PSL60 motorized fader
- Raspberry Pi Pico 2
- DRV8833 motor driver (breakout board)
- JST-XH connectors and cables
- M3 screws and heat-set inserts
- A GEN2 rail and 185-2W-1H case

## Documentation

**Full build guide, BOM, and software installation instructions:**

👉 **https://offworld-nexus-dhd.surge.sh**

The documentation covers everything from sourcing parts to flashing firmware.

## Source Code

Everything is open source — hardware (KiCad + STL), firmware (Rust/Embassy), and host software (Rust/Tokio):

👉 **https://github.com/Xowap/dhd**

## GEN2 Dependencies

You will also need to print:

- [GEN2-QL Rail — Double](https://www.printables.com/model/1052357-gen2-rails-185-standard/files)
- [185-2W-1H Case](https://www.printables.com/model/1658700-gen2-185-cases-all/files) (make sure it's the April 2026 or later version with the back wall hole)
