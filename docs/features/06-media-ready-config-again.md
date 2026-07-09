# Feature 06 — Config, media ready, Again

- `GET /v1/config` — edge arming, **ICE (STUN + optional TURN)**, pad map, flags
- ICE env: `CANIS_STUN_URLS`, `CANIS_TURN_URIS`, `CANIS_TURN_SECRET` (coturn REST) or static user/pass, `CANIS_FORCE_TURN`
- Portal fetches config before `RTCPeerConnection`; `canis-media` reads same TURN env
- Session starts `Negotiating`; both sides `media_ready` → `Active`
- `Again` extends `segment_deadline_at` without exceeding `max_end_at`
- Lab coturn: `deploy/coturn/`
