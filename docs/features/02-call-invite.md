# Feature 02 — Call invite

## What it does

A present dog presses **Call**. Cloud routes to the strongest **mutual bond** peer that is also present (K=1). Peer terminal runs a **dog-native lure** (LED + short audio). **No human push notification / accept.**

## Chain

```
Pad0 Call (MCU button frame)
  → EdgeAgent interprets Intent::Call
  → POST /v1/invites (device mTLS/token)
  → route_invite(bonds ∩ present)
  → Invite Ringing (25s TTL)
  → Peer GET /v1/invites/incoming → LureConfig
```

## Simulation

```bash
cargo run -p device-api -- --bind 127.0.0.1:8080 &
cargo run -p sim-dog -- --api http://127.0.0.1:8080 --scenario call
```

## Tests

```bash
cargo test -p bond -p session
cargo test -p e2e --test call_invite_full_chain
```
