# CanisLink Lab Kit (pair)

This is the **honest lab-shippable** target: two homes (or two rooms), two kits, one cloud API process, real WebRTC between media processes, durable SQLite.

## Bill of materials (per terminal)

| Item | Notes | ~USD |
|------|-------|------|
| Raspberry Pi 4/5 or CM4 4–8GB | Lab SBC | 60–120 |
| 1080p USB camera | Dog eye-line | 20–40 |
| USB speaker + mic (or headset dongle) | Full duplex later | 20 |
| 4× large arcade/silicone pads ~100mm | Call / Play / Again / Done | 20–40 |
| Force-sensing mat or bathroom scale load cell + HX711 | Presence | 15–40 |
| Optional VL53L0X ToF | Presence fusion | 5 |
| MCU (nRF52840 / Arduino-class) **or** GPIO buttons on Pi for lab | Pads | 10–20 |
| HDMI display 10–27" | Peer video later; not required for datachannel proof | 40–100 |
| Non-slip base / printed enclosure | See `hardware/enclosure/` | — |

**Minimum software lab (no soldering):** two PCs running `mcu-emu` + `canis-edge` + `canis-media`.

## Human install (30 minutes)

```bash
# 1. API (durable SQLite file in cwd)
cargo build -p device-api -p signaling -p steward -p sim-dog -p canis-media
./target/debug/device-api --bind 127.0.0.1:8080 &
./target/debug/signaling --bind 127.0.0.1:8081 &

# 2. Enroll two dogs
./target/debug/steward --api http://127.0.0.1:8080 enroll   # dog A
./target/debug/steward --api http://127.0.0.1:8080 enroll   # dog B
./target/debug/steward bond --dog-a <A> --dog-b <B>

# 3. Run social sim (control plane)
./target/debug/sim-dog --api http://127.0.0.1:8080 --scenario session

# 4. Real WebRTC between two media processes (replace UUIDs from a live session
#    or generate a test session uuid shared by both):
SESSION=$(uuidgen)
./target/debug/canis-media --session $SESSION --dog $(uuidgen) --role answerer &
sleep 0.5
./target/debug/canis-media --session $SESSION --dog $(uuidgen) --role offerer
```

## Emergency stop

```bash
./target/debug/steward estop --dog <DOG_UUID> --enabled true
```

## What is / isn’t included

| Included | Not included yet |
|----------|------------------|
| Durable enrollments, bonds, policies (SQLite) | Multi-region cloud |
| Dog social control plane | Factory mTLS PKI |
| Real WebRTC ICE + datachannel portal | Camera encode → remote display |
| Steward install CLI | Consumer mobile app store package |
| Emulated dual-dog e2e | Veterinary / large-scale field study |

