# Changelog

## 0.3.0-android-lab — 2026-07-09

### Anticipated gaps + Android video path

- Fail-path tests: offline peer, caller not present, sleep/rate limit, busy invite
- **portal-web**: browser/WebView dog video call UI (getUserMedia + WebRTC)
- **device-api** serves portal at `/portal/`
- **Android app** (`android/`): WebView wrapper, camera/mic permissions — **phone = dog camera+screen**
- `scripts/android_e2e.sh`: emulator install, control-plane call/accept, portal autostart, host chromium peer
- Docs: `ANTICIPATED_GAPS.md`

Honest: full two-way video quality on emulator is lab-grade (fake media host peer); not Play Store shipping.

## 0.2.0-lab

Lab-shippable software kit (SQLite, steward, WebRTC datachannel).

## 0.1.0-alpha

Control plane prototype.
