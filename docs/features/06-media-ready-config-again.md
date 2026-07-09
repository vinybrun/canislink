# Feature 06 — Config, media ready, Again

- `GET /v1/config` — edge arming, ICE stub, pad map, flags
- Session starts `Negotiating`; both sides `media_ready` → `Active`
- `Again` extends `segment_deadline_at` without exceeding `max_end_at`
