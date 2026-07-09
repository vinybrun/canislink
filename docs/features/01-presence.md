# Feature 01 — Presence

## What it does

Detect when a dog is at a terminal (mat force + ToF + motion), debounce enter/exit, and publish live presence to the cloud so the network knows who is *actually there*.

## Production chain

```
Dog on mat
  → load cell + ToF (hardware)
  → MCU sense frames @ 20 Hz (firmware/mcu-emu or real MCU)
  → UART bytes
  → canis-sense / EdgeAgent PresenceFilter (enter 800ms / exit 2500ms)
  → POST /v1/presence (device-api, Device auth)
  → PresenceStore TTL 10s
  → GET /v1/presence lists online dogs
```

## How to run simulation

```bash
# terminal 1
CANIS_DEVICE_SECRET=canis-dev-secret cargo run -p device-api -- --bind 127.0.0.1:8080

# terminal 2
cargo run -p sim-dog -- --api http://127.0.0.1:8080
```

## Tests

```bash
cargo test -p presence
cargo test -p protocol
cargo test -p mcu-emu
cargo test -p e2e --test presence_full_chain
```
