# CanisLink

Dog-to-dog social terminals: dogs Call / accept / end remote contact. **Humans never accept invites** — they only install, power, and emergency-stop.

## Status (honest)

| Milestone | State |
|-----------|--------|
| **Research prototype / control plane** | Done |
| **Lab-shippable software kit** | **This tree** — durable SQLite, steward CLI, real WebRTC datachannel path, sims & tests |
| **Customer-ready product** | **Not yet** — needs physical kits, camera video UX, hardened identity, field trials |

Version: see [`VERSION`](VERSION) (`0.2.0-lab`).

## Quick lab bring-up

```bash
cargo test --workspace
cargo build -p device-api -p signaling -p steward -p sim-dog -p canis-media

# durable API + signaling
./target/debug/device-api --bind 127.0.0.1:8080 &
./target/debug/signaling --bind 127.0.0.1:8081 &

# human install
./target/debug/steward enroll
./target/debug/steward enroll
./target/debug/steward bond --dog-a <A> --dog-b <B>

# dog social control plane (emulated hardware)
./target/debug/sim-dog --api http://127.0.0.1:8080 --scenario session

# real WebRTC between two media processes
SESSION=$(uuidgen)
./target/debug/canis-media --session $SESSION --dog $(uuidgen) --role answerer &
sleep 0.5
./target/debug/canis-media --session $SESSION --dog $(uuidgen) --role offerer
```

Ephemeral (no disk) for CI: `CANIS_EPHEMERAL=1` or `--ephemeral`.

## Docs

- Lab kit & install: [`docs/lab/LAB_KIT.md`](docs/lab/LAB_KIT.md)
- Definition of done: [`docs/lab/DEFINITION_OF_DONE.md`](docs/lab/DEFINITION_OF_DONE.md)
- Architecture: [`docs/architecture/canislink-system-architecture.md`](docs/architecture/canislink-system-architecture.md)
- Alpha runbook (control plane): [`docs/runbooks/alpha-ship.md`](docs/runbooks/alpha-ship.md)

## License

MIT OR Apache-2.0

## Local experiment (emulators)

```bash
./scripts/local_experiment.sh
```

Runs phone emulators (enroll/bond/estop) + embedded MCU/edge terminals + WebRTC media peers. Report: `docs/lab/experiment-report.json`. Details: [`docs/lab/LOCAL_EXPERIMENT.md`](docs/lab/LOCAL_EXPERIMENT.md).
