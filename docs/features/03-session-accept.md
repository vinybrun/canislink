# Feature 03 — Accept + session (media stub)

## What it does

Callee dog **engages any pad** while `RingingIn` → cloud accepts invite → `Session` becomes **Active**. WebRTC role assigned (initiator=offerer). Real AV is stubbed; control plane is production-shaped.

Walk-away mid-ring ignores; Done ends Active session.

## Chain

```
RingingIn + pad engage
  → POST /v1/invites/{id}/accept
  → accept_invite pure check (callee, present, not expired)
  → SessionStore Active
  → webrtc_role offerer|answerer
  → GET /v1/sessions/active
  → POST /v1/sessions/{id}/end { reason: done|walk_away }
```

## Sim

```bash
cargo run -p device-api -- --bind 127.0.0.1:8080 &
cargo run -p sim-dog -- --api http://127.0.0.1:8080 --scenario session
cargo run -p sim-dog -- --api http://127.0.0.1:8080 --scenario end
```
