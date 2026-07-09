# Presence mat + sensing (Feature: Presence)

## Bill of materials (sensing only)

| Part | Role | Notes |
|------|------|-------|
| Load cell 50–200 kg + HX711 | Force F | Under mat platform |
| VL53L0X ToF | Distance d | Mast / console aiming at mat |
| Optional PIR | Motion | Side channel |
| nRF52840 / STM32G0 | MCU | 50 Hz force, 20 Hz ToF, UART 115200 |
| Silicone top mat | Grip | Washable cover |

## Thresholds (default firmware/edge)

| Param | Value |
|-------|-------|
| F_min | 25 N |
| ToF near/far | 50–900 mm |
| Enter debounce | 800 ms |
| Exit debounce | 2500 ms |
| Sense frame rate | 20 Hz |
| Cloud TTL | 10 s |
| Publish interval | 2 s |

## Wiring (MCU)

| Signal | MCU pin (example nRF52840) |
|--------|----------------------------|
| HX711 DT | P0.03 |
| HX711 SCK | P0.04 |
| VL53L0X SDA/SCL | P0.26 / P0.27 |
| UART TX→SBC | P0.06 |
| UART RX←SBC | P0.08 |
| Pad1–4 | P0.11–P0.14 |
| LED strip data | P0.15 |

## Safety

- Overload band (>450 N) → telemetry only, still present if in ToF band
- MCU watchdog independent of SBC
- No cloud dependency for local safe state
