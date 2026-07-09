# CanisLink

**Dog-to-dog social portal network** — standalone terminals where dogs initiate, accept, sustain, and end remote social contact with **no human in the session protocol**.

**Version:** `0.1.0-alpha` (base control plane ready to ship)

## Repo

Public monorepo: https://github.com/vinybrun/canislink

## What ships in alpha

| # | Feature | Status |
|---|---------|--------|
| 01 | Presence | ✅ |
| 02 | Call invite + lure | ✅ |
| 03 | Session accept | ✅ |
| 04 | Session end | ✅ |
| 05 | Policy + steward e-stop | ✅ |
| 06 | Config + media_ready + Again | ✅ |
| 07 | Ship packaging + e2e gate | ✅ |
| — | WebRTC live AV pixels | 🔜 next |

Humans are infrastructure only (install, power, billing, emergency stop). They never accept invites.

## Quick start

```bash
cargo test --workspace
cargo build -p device-api -p sim-dog

./target/debug/device-api --bind 127.0.0.1:8080 &
./target/debug/sim-dog --api http://127.0.0.1:8080 --scenario ship
```

## Docs

- Architecture: [`docs/architecture/canislink-system-architecture.md`](docs/architecture/canislink-system-architecture.md)
- Alpha runbook: [`docs/runbooks/alpha-ship.md`](docs/runbooks/alpha-ship.md)
- Features: [`docs/features/`](docs/features/)

## Hardware / 3D

- `hardware/enclosure/*.scad` — mat, pads, lure bezel
- `hardware/electronics/` — presence, lure, session LEDs
- `hardware/bom/prototype.csv`
- `firmware/mcu-emu` + `firmware/mcu/`

## License

MIT OR Apache-2.0
