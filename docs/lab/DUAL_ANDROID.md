# Dual Android dog portal peers (lab)

## Recommended: same emulator, two packages

Two product flavors install side-by-side:

| Flavor | applicationId |
|--------|----------------|
| peer1 | `com.canislink.portal` |
| peer2 | `com.canislink.portal.peer2` |

```bash
./scripts/start_canis_avd.sh CanisLab
MODE=same_device ./scripts/dual_android_e2e.sh
# expects DUAL_ANDROID_E2E_OK + docs/lab/dual-android-e2e-report.json
```

Both load `http://127.0.0.1:PORT/portal/` via **adb reverse** (secure context → getUserMedia).

## Optional: two AVDs

```bash
./scripts/start_canis_avd.sh CanisLab 5554
FORCE_NEW=1 ./scripts/start_canis_avd.sh CanisLab2 5556
MODE=dual_avd ./scripts/dual_android_e2e.sh
```

**Honest note:** dual QEMU instances are memory/CPU heavy and may drop offline under install/WebRTC load. Prefer `same_device` in CI-like lab hosts.

## What “ok” means

- Control plane session `Active`, both media_ready
- Both portals autostart WebRTC roles
- At least one MEDIA_PATH (getUserMedia or lab_canvas)
- Signaling open observed

Full `pc connected` / remote track on both under same-device backgrounding is **not** guaranteed; use two physical phones or stable dual AVD for that bar.
