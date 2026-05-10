# Usage

Now that the software is installed:

- Make sure nothing is obstructing your knob. The calibration process will start immediately when the host program turns on
- Look for the "DHD Host" in your menu to start the program

You will get a small icon appearing in your systray, which tells you the state at all times.

## Calibration

When starting for the first time a calibration process happens. Don't worry, the results are saved on the device itself and remembered after reboot, so this will only happen once. You can however force a re-calibration for any reason (new knob with different weight, first calibration got interfered with, etc) by right-clicking the systray icon and selecting "Force Calibration".

## Scale Inversion

Another thing which you will probably need to do if you have the same setup as me, is that the potentiometer is mounted with 0% on the right and 100% on the left, which in many cultures is counter-intuitive for volume setting. In which case you can opt to invert the scale, and you will be left-to-right.

## Systray States

The systray icon reflects the current state:

| Icon | State | Meaning |
|------|-------|---------|
| ![](../img/dhd-offline.svg){ width="24" } | Offline | Device not connected |
| ![](../img/dhd-init.svg){ width="24" } | Init | Connecting / handshake in progress |
| ![](../img/dhd-calib.svg){ width="24" } | Calibration | Auto-calibration running |
| ![](../img/dhd-ready.svg){ width="24" } | Ready | Standby, waiting for input |
| ![](../img/dhd-hand.svg){ width="24" } | Hand | Physically driven (user moving fader) |
| ![](../img/dhd-auto.svg){ width="24" } | Auto | Logically driven (computer moving fader) |
| ![](../img/dhd-error.svg){ width="24" } | Error | Failsafe / communication error |

## Signal Control

You can also control the daemon via Unix signals:

| Signal | Action |
|--------|--------|
| `SIGUSR1` | Force hard recalibration |
| `SIGUSR2` | Toggle scale inversion |

```bash
kill -USR1 $(pgrep dhd)
```

## CLI Reference

```
dhd [OPTIONS] [COMMAND]
```

### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--log-level <LEVEL>` | `-l` | Set device log level (`error`, `warn`, `info`, `debug`, `trace`) | `info` |
| `--systray` | `-s` | Enable system tray icon and detach from terminal | off |
| `--help` | `-h` | Print help | — |

The log level can also be set via the `DHD_LOG` environment variable.

### Subcommands

| Command | Description |
|---------|-------------|
| `run` | Run the DHD host (default if no subcommand given) |
| `install` | Install the DHD host to autostart and system menu |

### Examples

```bash
# Run in foreground with debug logging
dhd -l debug

# Run as background daemon with systray
dhd --systray

# Install to autostart
dhd install
```

## Hardware Compatibility

The software makes few assumptions regarding the hardware, this way you can potentially hook up another fader out-of-the-box (modulo the exact screw locations) and it's likely that it will work decently. No alternative has been tested however.
