# CanisLink MCU firmware (production target)

Target: nRF52840 or STM32G0 class MCU.

## Responsibilities

- Sample force (ADC / load cell amp) at 50 Hz
- Sample ToF (VL53L0X or similar) at 20 Hz
- Debounce buttons (50 ms) on 4 pads
- Stream `Sense` frames at 20 Hz over UART 115200 8N1
- Apply LED commands from SBC
- Independent watchdog: if SBC heartbeat lost > 3 s → LEDs off, safe state

## Protocol

See `crates/protocol/src/mcu.rs` and `docs/protocols/mcu-uart.md`.

## Emulator

Host-side realistic emulator: `firmware/mcu-emu` (used in CI and lab without hardware).
