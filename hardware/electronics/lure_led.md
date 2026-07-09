# Lure LED / audio (Call receive path)

When `RingingIn`:
- LED pattern `slow_pulse_blue` on console ring (canine-visible blue)
- Audio chirp ≤ 2 s, max 3 repeats with backoff
- **Never** phone notification as accept path

## Wiring

| Function | Notes |
|----------|-------|
| WS2812 ring 16 px | MCU pin LED data |
| Small speaker / piezo | Via amp on console |

OpenSCAD: `hardware/enclosure/lure_bezel.scad`
