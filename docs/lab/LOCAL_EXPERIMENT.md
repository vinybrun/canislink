# Local experiment (emulators)

## What is emulated

| Role | Emulator | Real component |
|------|----------|----------------|
| Steward phone (human) | `PhoneEmu` in `lab-experiment` | Install / bond / e-stop only |
| Dog terminal MCU | `mcu-emu` | Pads + force mat UART frames |
| Edge SBC agent | `canis-edge` | Presence, call, accept, session |
| Media plane | `canis-media` ×2 | Real WebRTC ICE + portal datachannel |
| Cloud | `device-api` + `signaling` | Durable SQLite + signal rooms |

Humans **never** accept dog invites in this experiment.

## Run

```bash
./scripts/local_experiment.sh
```

Optional:

```bash
API_PORT=18080 SIG_PORT=18081 REPORT=docs/lab/experiment-report.json \
  ./scripts/local_experiment.sh
```

## Pass criteria

Report JSON `ok: true` with steps:

1. `api_health`
2. `phone_enroll` (two phones)
3. `phone_bond`
4. `embedded_presence` (both dogs on mats)
5. `dog_a_call`
6. `dog_b_lure` (no phone push)
7. `dog_b_accept` (pad engage)
8. `session_active`
9. `dog_a_again`
10. `webrtc_portal`
11. `dog_a_done`
12. `phone_estop`

## Logs

- `/tmp/canis-exp-api.log`
- `/tmp/canis-exp-sig.log`
- `docs/lab/experiment-report.json`
