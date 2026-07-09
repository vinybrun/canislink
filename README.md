# CanisLink

**Dog-to-dog social portal network** — standalone terminals where dogs initiate, accept, sustain, and end remote social contact with **no human in the session protocol**.

Humans are infrastructure only (purchase, power, install, billing, emergency stop).

## Repo

Public monorepo: Rust cloud + edge, MCU emulator/firmware reference, hardware docs, OpenSCAD enclosure models.

## Features

| # | Feature | Status | Doc |
|---|---------|--------|-----|
| 01 | **Presence** (mat → MCU → edge → cloud) | implemented | [docs/features/01-presence.md](docs/features/01-presence.md) |
| 02 | **Call invite** | implemented | [docs/features/02-call-invite.md](docs/features/02-call-invite.md) |
| 03 | **Session accept** | implemented | [docs/features/03-session-accept.md](docs/features/03-session-accept.md) |
| 04 | **Session end** | implemented | [docs/features/04-session-end.md](docs/features/04-session-end.md) |
| 03 | Accept + session | planned | — |

## Quick start

```bash
cargo test --workspace
cargo test -p e2e --test presence_full_chain

# Full dual-dog simulation
cargo run -p device-api -- --bind 127.0.0.1:8080 &
cargo run -p sim-dog -- --api http://127.0.0.1:8080
```

## Architecture

- [`docs/architecture/canislink-system-architecture.md`](docs/architecture/canislink-system-architecture.md)

## Hardware / 3D

- Electronics: `hardware/electronics/presence_mat.md`
- OpenSCAD: `hardware/enclosure/*.scad`
- BOM: `hardware/bom/prototype.csv`

## License

MIT OR Apache-2.0
