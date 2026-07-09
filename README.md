# CanisLink

Dog-to-dog social portal network: standalone terminals where dogs initiate, accept, sustain, and end remote social contact — **with no human in the session protocol**.

Humans are infrastructure only (purchase, power, install, billing, emergency stop).

## Status

Founding architecture approved (Draft 0.3.0). Implementation follows the PR plan in:

- [`docs/architecture/canislink-system-architecture.md`](docs/architecture/canislink-system-architecture.md)

## Architecture (one monorepo)

| Area | Path |
|------|------|
| Shared Rust crates | `crates/` |
| Cloud services | `services/` |
| Edge (SBC) binaries | `edge/` |
| MCU firmware | `firmware/mcu/` |
| Hardware docs / BOM | `hardware/` |
| Deploy (compose, CA, TURN) | `deploy/` |
| Tools (sim-dog, provision) | `tools/` |
| E2E / conformance | `tests/` |
| Architecture & protocols | `docs/` |

## Product thesis

```text
Dog A present → Call/Play → lure on eligible peer terminal
  → Dog B engages (accept) or ignores (refuse)
  → WebRTC portal session
  → Done / walk-away / segment expiry ends
```

No owner push-to-accept. Receiver UX is dog-native (LED + short audio).

## Quick start (dev)

```bash
# Requires: Rust stable (see rust-toolchain.toml), Docker (for later compose stack)
cargo check --workspace
cargo test --workspace
```

Cloud + dual-terminal simulation land in early PRs (`tools/sim-dog`). Hardware lab is later.

## License

TBD — all rights reserved until a license is chosen.
