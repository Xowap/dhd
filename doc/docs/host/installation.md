# Installation

For now the software distribution is not fantastic, in the sense that you will also have to build it yourself.

First of all, it only works for Linux. So for other operating systems, you can probably vibe-code it pretty easily given that most of the code is portable, however yeah that's on you.

## Prerequisites

The things you'll need to get started are:

- `git`, of course
- and `rust`, so for example you can get [`rustup`](https://rustup.rs/)

## Getting the Code

Start by cloning the repo:

```bash
git clone https://github.com/Xowap/dhd.git
cd dhd
```

Then install the toolchain for cross-compilation:

```bash
rustup target add thumbv8m.main-none-eabihf
```

Then there are two components:

- The `firmware`, which runs on the device
- And the `host`, which runs on your computer and that is the Linux-only part

## Flashing the Firmware

So now, let's flash this firmware.

Look at your Raspberry Pi Pico 2. It has a `BOOTSEL` button. Maintain it pressed while plugging it to the USB port of your computer. This puts the device in "flashable" mode.

Subsequently, build and flash the firmware:

```bash
cd firmware
cargo run --release
cd ..
```

This will transfer the firmware to the device and then it should immediately turn on. Of course it does nothing on its own so you need to start the host software.

## Installing the Host

The host works in CLI mode, however if you intend on using it I recommend using the self-installer.

```bash
cd host
cargo run --release -- install
```

Doing so will:

- Install the binary in `~/.local/bin/dhd`
- Configure the application to appear in your menu and to start automatically at system launch

The installed files are listed, so if you want to cleanup afterwards it's very simple to do because you just need to delete them.
