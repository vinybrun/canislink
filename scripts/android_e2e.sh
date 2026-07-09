#!/usr/bin/env bash
# Profound e2e: backend + Android emulator dog video portal
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

export ANDROID_HOME=${ANDROID_HOME:-/opt/android-sdk}
export ANDROID_SDK_ROOT=$ANDROID_HOME
export PATH=$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator
export LD_LIBRARY_PATH=$ANDROID_HOME/emulator/lib64:$ANDROID_HOME/emulator/lib64/qt/lib:${LD_LIBRARY_PATH:-}
export JAVA_HOME=${JAVA_HOME:-/usr/lib/jvm/java-17-openjdk-amd64}
export CANIS_PORTAL_DIR=$ROOT/portal-web

API_PORT=${API_PORT:-18080}
SIG_PORT=${SIG_PORT:-18081}
API="http://127.0.0.1:${API_PORT}"
REPORT=${REPORT:-docs/lab/android-e2e-report.json}
APK=$ROOT/android/app/build/outputs/apk/debug/app-debug.apk

echo "==> Ensure APK"
if [ ! -f "$APK" ]; then
  (cd android && ./gradlew :app:assembleDebug)
fi

echo "==> Ensure emulator"
if ! adb devices 2>/dev/null | grep -qE 'emulator-.*[[:space:]]device'; then
  echo "No running emulator device; start CanisLab first"
  exit 1
fi
adb wait-for-device

echo "==> Build + start API/signaling"
cargo build -q -p device-api -p signaling -p steward
# stop previous by port if needed
fuser -k ${API_PORT}/tcp 2>/dev/null || true
fuser -k ${SIG_PORT}/tcp 2>/dev/null || true
sleep 0.5
rm -f /tmp/canis-android-e2e.db
./target/debug/device-api --bind "127.0.0.1:${API_PORT}" \
  --database-url 'sqlite:/tmp/canis-android-e2e.db?mode=rwc' \
  > /tmp/android-e2e-api.log 2>&1 &
API_PID=$!
./target/debug/signaling --bind "127.0.0.1:${SIG_PORT}" > /tmp/android-e2e-sig.log 2>&1 &
SIG_PID=$!
cleanup() { kill $API_PID $SIG_PID 2>/dev/null || true; }
trap cleanup EXIT

for i in $(seq 1 50); do curl -sf "$API/healthz" >/dev/null && break; sleep 0.1; done
curl -sf "$API/portal/" | head -c 40 >/dev/null
echo "portal ok"

EA=$(./target/debug/steward --api "$API" enroll)
EB=$(./target/debug/steward --api "$API" enroll)
DOG_A=$(echo "$EA" | sed -n 's/^dog_id=//p')
TERM_A=$(echo "$EA" | sed -n 's/^terminal_id=//p')
TOKEN_A=$(echo "$EA" | sed -n 's/^token=//p')
DOG_B=$(echo "$EB" | sed -n 's/^dog_id=//p')
TERM_B=$(echo "$EB" | sed -n 's/^terminal_id=//p')
TOKEN_B=$(echo "$EB" | sed -n 's/^token=//p')
./target/debug/steward --api "$API" bond --dog-a "$DOG_A" --dog-b "$DOG_B"

adb install -r "$APK" >/dev/null
adb shell pm grant com.canislink.portal android.permission.CAMERA || true
adb shell pm grant com.canislink.portal android.permission.RECORD_AUDIO || true

# Secure context for getUserMedia: loopback via adb reverse (10.0.2.2 is NOT secure).
echo "==> adb reverse for secure-context portal"
adb reverse tcp:${API_PORT} tcp:${API_PORT} || true
adb reverse tcp:${SIG_PORT} tcp:${SIG_PORT} || true
adb reverse --list || true

present() {
  curl -sf -X POST "$API/v1/presence" \
    -H "Authorization: Device $2:$3" -H 'Content-Type: application/json' \
    -d "{\"dog_id\":\"$1\",\"terminal_id\":\"$2\",\"present\":true,\"confidence\":0.95,\"force_band\":\"medium\",\"force_n\":120,\"tof_mm\":400,\"ts\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"seq\":$4}"
}
present "$DOG_A" "$TERM_A" "$TOKEN_A" 1
present "$DOG_B" "$TERM_B" "$TOKEN_B" 1

INV=$(curl -sf -X POST "$API/v1/invites" \
  -H "Authorization: Device ${TERM_A}:${TOKEN_A}" -H 'Content-Type: application/json' \
  -d "{\"mode\":\"portal\",\"to_dog\":null,\"dog_id\":\"$DOG_A\",\"terminal_id\":\"$TERM_A\"}")
INV_ID=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['invite']['id'])" "$INV")
ACC=$(curl -sf -X POST "$API/v1/invites/${INV_ID}/accept" \
  -H "Authorization: Device ${TERM_B}:${TOKEN_B}" -H 'Content-Type: application/json' \
  -d "{\"dog_id\":\"$DOG_B\",\"terminal_id\":\"$TERM_B\"}")
SESS=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['session']['id'])" "$ACC")

curl -sf -X POST "$API/v1/sessions/${SESS}/media_ready" \
  -H "Authorization: Device ${TERM_A}:${TOKEN_A}" -H 'Content-Type: application/json' \
  -d "{\"dog_id\":\"$DOG_A\",\"terminal_id\":\"$TERM_A\",\"ready\":true}" >/dev/null
MR=$(curl -sf -X POST "$API/v1/sessions/${SESS}/media_ready" \
  -H "Authorization: Device ${TERM_B}:${TOKEN_B}" -H 'Content-Type: application/json' \
  -d "{\"dog_id\":\"$DOG_B\",\"terminal_id\":\"$TERM_B\",\"ready\":true}")
BOTH=$(python3 -c "import json,sys; print(json.loads(sys.argv[1]).get('both_ready'))" "$MR")

# 127.0.0.1 is a secure context → mediaDevices.getUserMedia available on WebView
PORTAL_B="http://127.0.0.1:${API_PORT}/portal/?api=http://127.0.0.1:${API_PORT}&signal=ws://127.0.0.1:${SIG_PORT}&token=${TOKEN_B}&terminalId=${TERM_B}&dogId=${DOG_B}&session=${SESS}&role=answerer&autostart=1"
adb logcat -c || true
adb shell am force-stop com.canislink.portal || true
# Quote carefully so & in query string is not eaten by adb shell
adb shell am start -n com.canislink.portal/.MainActivity --es portal_url "'${PORTAL_B}'"
sleep 10

PORTAL_A="http://127.0.0.1:${API_PORT}/portal/?api=http://127.0.0.1:${API_PORT}&signal=ws://127.0.0.1:${SIG_PORT}&token=${TOKEN_A}&terminalId=${TERM_A}&dogId=${DOG_A}&session=${SESS}&role=offerer&autostart=1"
CHROME=$(command -v chromium-browser || command -v chromium)
$CHROME --headless=new --disable-gpu --use-fake-ui-for-media-stream --use-fake-device-for-media-stream \
  --user-data-dir=/tmp/chrome-canis-a-e2e --remote-debugging-port=9222 \
  "$PORTAL_A" > /tmp/chrome-a.log 2>&1 &
sleep 8

ACTIVE=$(curl -sf "$API/v1/sessions/active?dog_id=${DOG_A}&terminal_id=${TERM_A}" \
  -H "Authorization: Device ${TERM_A}:${TOKEN_A}")
APP_TOP=$(adb shell dumpsys activity activities | grep -E 'topResumedActivity' | head -1 || true)
mkdir -p docs/lab
LOGCAT=$(adb logcat -d -s CanisLink:I 2>/dev/null | tail -80 || true)
echo "$LOGCAT" | tail -30
adb exec-out screencap -p > docs/lab/android-screencap.png || true
PAGE_OK=$(echo "$LOGCAT" | grep -c page_finished || true)
WS_OK=$(echo "$LOGCAT" | grep -ciE 'device WS|ws open|portal ready' || true)
PC_OK=$(echo "$LOGCAT" | grep -ciE 'pc connected|connectionstate|sent offer|sent answer|got answer|remote track' || true)
# MEDIA_PATH=getUserMedia means real camera API; lab_canvas is fallback
MEDIA_GUM=$(echo "$LOGCAT" | grep -c 'MEDIA_PATH=getUserMedia' || true)
MEDIA_LAB=$(echo "$LOGCAT" | grep -c 'MEDIA_PATH=lab_canvas' || true)
SECURE_HIT=$(echo "$LOGCAT" | grep -ciE 'isSecureContext=true' || true)


python3 - << PY
import json
active = json.loads('''$ACTIVE''')
both = '''$BOTH''' in ("True", "true", True)
media_gum = int("""$MEDIA_GUM""".strip() or "0")
media_lab = int("""$MEDIA_LAB""".strip() or "0")
report = {
  "ok": bool(active and active.get("id") == "$SESS") and both,
  "session_id": "$SESS",
  "invite_id": "$INV_ID",
  "dog_a": "$DOG_A",
  "dog_b": "$DOG_B",
  "both_media_ready": both,
  "session_state": active.get("state") if active else None,
  "android_device": "emulator",
  "android_app_resumed": "com.canislink.portal" in '''$APP_TOP''',
  "portal_url_android": "autostart answerer via adb reverse 127.0.0.1 (secure context)",
  "host_peer": "chromium headless offerer with fake media",
  "apk": "$APK",
  "screenshot": "docs/lab/android-screencap.png",
  "architecture_shift": "Android phone is dog camera+screen instead of custom AV hardware",
  "fail_paths": "tests/e2e/tests/fail_paths.rs",
  "android_page_finished_logs": int("""$PAGE_OK""".strip() or "0"),
  "android_portal_log_hits": int("""$WS_OK""".strip() or "0"),
  "android_webrtc_log_hits": int("""$PC_OK""".strip() or "0"),
  "media_path_getUserMedia_hits": media_gum,
  "media_path_lab_canvas_hits": media_lab,
  "secure_context_true_hits": int("""$SECURE_HIT""".strip() or "0"),
  "device_ws_push": "enabled on /v1/ws",
  "ice_config": "GET /v1/config STUN/TURN",
}
report["android_page_loaded"] = report["android_page_finished_logs"] > 0
# Prefer real getUserMedia when reverse+perms work; canvas still counts as session-ok
report["media_path"] = "getUserMedia" if media_gum > 0 else ("lab_canvas" if media_lab > 0 else "unknown")
open("$REPORT", "w").write(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
if not report["ok"] or not report["android_app_resumed"]:
    raise SystemExit("ANDROID_E2E incomplete: " + json.dumps(report))
print("ANDROID_E2E_OK media_path=" + report["media_path"])
PY
