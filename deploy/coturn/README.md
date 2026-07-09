# Lab coturn (TURN)

CanisLink distributes ICE via `GET /v1/config`:

| Env on `device-api` | Purpose |
|---------------------|---------|
| `CANIS_STUN_URLS` | Comma-separated STUN URLs |
| `CANIS_TURN_URIS` | e.g. `turn:127.0.0.1:3478?transport=udp` |
| `CANIS_TURN_SECRET` | Coturn `static-auth-secret` → mint ephemeral REST creds |
| `CANIS_TURN_USERNAME` / `CANIS_TURN_CREDENTIAL` | Static creds if secret unset |
| `CANIS_FORCE_TURN` | Portal uses `iceTransportPolicy=relay` |

## Quick start (docker)

```bash
docker run --rm -p 3478:3478/udp -p 3478:3478 \
  -v "$PWD/deploy/coturn/turnserver.conf:/etc/coturn/turnserver.conf:ro" \
  coturn/coturn -c /etc/coturn/turnserver.conf
```

Then start device-api with:

```bash
export CANIS_TURN_URIS='turn:127.0.0.1:3478?transport=udp'
export CANIS_TURN_SECRET='canis-lab-turn-secret'
export CANIS_STUN_URLS='stun:stun.l.google.com:19302'
```

Portal / Android WebView fetch config before `RTCPeerConnection`.
`canis-media` accepts the same `CANIS_TURN_*` env for peer lab processes.

**Honest scope:** shipping a public TURN fleet, TLS (TURNS), and production secrets rotation is **not** claimed ready — this is lab NAT traversal plumbing.
