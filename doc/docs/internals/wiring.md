# Wiring Reference

This document describes the physical connections between the Raspberry Pi Pico 2
(RP2350) and the hardware components, as defined in the KiCad schematic
(`hardware/kicad/dhd.kicad_sch`).

## Potentiometer (J2 — 4-pin JST-XH)

The Bourns PSL60 fader has 4 pins labeled "1 2 T 3" on the component.

| Fader Pin | Connector Pin | Pico Connection | Function |
|-----------|---------------|-----------------|----------|
| 1 | J2-1 | GP26 / ADC0 (Pin 31) | Wiper — analog position signal |
| 2 | J2-2 | — | Touch sense (unused, not connected) |
| T | — | — | (label on fader, not a separate pin) |
| 3 | J2-3 | AGND (Pin 33) | Ground reference (analog) |
| — | J2-4 | 3.3V (Pin 36) | Positive voltage reference |

!!! note "Analog ground"
    The schematic connects the pot ground to **AGND** (Pin 33) rather than a
    generic GND pin. This is best practice for clean ADC readings — it avoids
    noise coupling from digital circuits.

## Motor Driver (DRV8833 — dual 6-pin headers)

The DRV8833 breakout board is represented as two 6-pin connectors in the
schematic (left and right pin rows).

| DRV8833 Pin | Pico Connection | Purpose |
|-------------|-----------------|---------|
| VM / VCC | VBUS (Pin 40, 5V) | Motor power from USB |
| GND | GND (Pin 38) | Common ground |
| IN1 | GP14 (Pin 19) | PWM forward control |
| IN2 | GP15 (Pin 20) | PWM reverse control |
| EEP / nSLEEP | 3.3V (Pin 36) | Enable (must be HIGH) |
| OUT1 | J1-2 (Motor pin) | Motor power output |
| OUT2 | J1-1 (Motor pin) | Motor power output |

**Unused DRV8833 pins:** IN3, IN4, OUT3, OUT4 (second channel), ULT/nFAULT.

## Motor (J1 — 2-pin JST-XH)

| Pin | Connection | Note |
|-----|------------|------|
| J1-1 | DRV8833 OUT2 | Polarity doesn't matter |
| J1-2 | DRV8833 OUT1 | Polarity doesn't matter |

## USB Connection

The Pico's USB port provides:

1. **Power** — 5V via VBUS to the motor driver, 3.3V regulated for logic
2. **Data** — CDC-ACM serial for the JSON protocol
3. **Flashing** — via BOOTSEL mode for firmware updates

## Firmware Pin Configuration

Confirmed in `firmware/src/main.rs`:

```rust
// PWM motor control on Slice 7
let pwm = Pwm::new_output_ab(p.PWM_SLICE7, p.PIN_14, p.PIN_15, pwm_c);

// ADC input
let channel = Channel::new_pin(p.PIN_26, embassy_rp::gpio::Pull::None);
```

## Schematic Overview

```
                    ┌─────────────────────┐
                    │  Raspberry Pi Pico 2 │
                    │                     │
    3.3V (Pin 36) ─┤─── EEP (DRV8833)    │
                    │                     │
   VBUS (Pin 40) ─┤─── VM  (DRV8833)    │
                    │                     │
    GND (Pin 38) ──┤─── GND (DRV8833)    │
                    │                     │
   GP14 (Pin 19) ──┤─── IN1 (DRV8833)    │──── OUT1 ──┐
                    │                     │             │ Motor
   GP15 (Pin 20) ──┤─── IN2 (DRV8833)    │──── OUT2 ──┘
                    │                     │
   GP26 (Pin 31) ──┤─── Wiper (Pot)      │
                    │                     │
   AGND (Pin 33) ──┤─── GND   (Pot)      │
                    │                     │
    3.3V (Pin 36) ─┤─── VCC   (Pot)      │
                    │                     │
         USB ──────┤ Power + CDC-ACM      │
                    └─────────────────────┘
```
