# Feature 04 — Session end (Done / walk-away)

Included with session accept:

- **Done pad** while `InSession` → `POST /v1/sessions/{id}/end` reason `done`
- **Walk-away** (presence lost while InSession) → reason `walk_away`
- Edge returns to `IdlePresent` / `IdleEmpty`

Sim: `sim-dog --scenario end`
