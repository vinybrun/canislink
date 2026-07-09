# Alpha ship runbook

## What ships in 0.1.0-alpha

Dog-to-dog **control plane** on standalone terminals:

1. Presence (mat → MCU → edge → cloud)
2. Call invite (mutual bonds, K=1, dog-native lure)
3. Accept (pad engage) → session Negotiating → media_ready → Active
4. Again (soft segment extend)
5. Done / walk-away end
6. Steward emergency_stop + social_disabled + policy
7. GET /v1/config for edge arming

**Not yet production AV:** WebRTC GStreamer path is stubbed (`canis-media` process + media_ready handshake). Portal *control* is complete; pixels come next.

## Humans out of session path

Steward may never accept invites. Accept = dog pad engage only.

## Run locally

```bash
cargo build -p device-api -p sim-dog
./target/debug/device-api --bind 127.0.0.1:8080 &
./target/debug/sim-dog --api http://127.0.0.1:8080 --scenario ship
```

## Steward

```bash
curl -X POST http://127.0.0.1:8080/v1/steward/estop \
  -H "Authorization: Steward canis-steward-secret" \
  -H "Content-Type: application/json" \
  -d '{"dog_id":"<uuid>","enabled":true}'
```

## Tests

```bash
cargo test --workspace
cargo test -p e2e --test ship_base_full_chain
```
