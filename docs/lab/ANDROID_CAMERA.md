# Android / emulator camera path (lab)

## Problem

WebView `navigator.mediaDevices.getUserMedia` requires a **secure context**.

| Portal origin | Secure? | getUserMedia |
|---------------|---------|--------------|
| `http://10.0.2.2:PORT` (emulator→host) | **No** | usually missing → lab canvas fallback |
| `http://127.0.0.1:PORT` after `adb reverse` | **Yes** | real path on WebView |
| `https://…` with trusted cert | Yes | real path |

## Lab procedure

```bash
# 1) AVD with virtual camera (no webcam)
./scripts/start_canis_avd.sh CanisLab
adb wait-for-device

# 2) Reverse host control/signaling ports into the emulator loopback
adb reverse tcp:18080 tcp:18080
adb reverse tcp:18081 tcp:18081

# 3) Runtime permissions
adb shell pm grant com.canislink.portal android.permission.CAMERA
adb shell pm grant com.canislink.portal android.permission.RECORD_AUDIO

# 4) Load portal on 127.0.0.1 (not 10.0.2.2)
# scripts/android_e2e.sh does this automatically
```

## Logs to expect

```
media probe isSecureContext=true mediaDevices=true origin=http://127.0.0.1:18080
MEDIA_PATH=getUserMedia av tracks=2
```

Fallback (still valid for WebRTC negotiation without camera):

```
MEDIA_PATH=lab_canvas fallback
```

Force canvas: `?lab_cam=1` on portal URL.

## Honest limits

- Emulator **virtualscene** is not a real dog-facing lens; it proves the **API path**.
- Physical phone: same app + HTTPS or cleartext-localhost only if you tunnel.
- Production should use HTTPS (or WSS) and proper cert pinning — not claimed here.
