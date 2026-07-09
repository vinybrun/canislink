#!/usr/bin/env bash
# Dual Android dog-to-dog video portal e2e (lab).
#
# Modes:
#   MODE=same_device (default, reliable) — two app flavors on one emulator
#     packages: com.canislink.portal + com.canislink.portal.peer2
#   MODE=dual_avd — two emulators (SERIAL_A + SERIAL_B); heavier / flakier on low RAM
#
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
REPORT=${REPORT:-docs/lab/dual-android-e2e-report.json}
MODE=${MODE:-same_device}
SERIAL_A=${SERIAL_A:-emulator-5554}
SERIAL_B=${SERIAL_B:-emulator-5556}
PKG_A=com.canislink.portal
PKG_B=com.canislink.portal.peer2
APK_A=$ROOT/android/app/build/outputs/apk/peer1/debug/app-peer1-debug.apk
APK_B=$ROOT/android/app/build/outputs/apk/peer2/debug/app-peer2-debug.apk

echo "==> Mode=$MODE"
if [ "$MODE" = "dual_avd" ]; then
  PKG_B=com.canislink.portal
  APK_B=$APK_A
  echo "==> Dual devices"
  adb devices -l
  for s in "$SERIAL_A" "$SERIAL_B"; do
    if ! adb devices | grep -qE "^${s}[[:space:]]+device"; then
      echo "missing device $s — start both AVDs or use MODE=same_device"
      exit 1
    fi
    adb -s "$s" wait-for-device
  done
else
  SERIAL_B=$SERIAL_A
  echo "==> Single device dual packages ($SERIAL_A)"
  adb devices -l
  if ! adb devices | grep -qE "^${SERIAL_A}[[:space:]]+device"; then
    echo "missing $SERIAL_A"
    exit 1
  fi
  adb -s "$SERIAL_A" wait-for-device
fi

echo "==> Ensure dual peer APKs"
if [ ! -f "$APK_A" ] || [ ! -f "$APK_B" ]; then
  (cd android && ./gradlew :app:assemblePeer1Debug :app:assemblePeer2Debug)
fi

echo "==> Build backend"
cargo build -q -p device-api -p signaling -p steward
for pid in $(ps -eo pid,cmd | awk '/target\/debug\/(device-api|signaling)( |$)/{print $1}'); do
  echo "freeing lab process pid=$pid"
  kill "$pid" 2>/dev/null || true
done
sleep 0.8
rm -f /tmp/canis-dual-android.db
./target/debug/device-api --bind "127.0.0.1:${API_PORT}" \
  --database-url 'sqlite:/tmp/canis-dual-android.db?mode=rwc' \
  > /tmp/dual-android-api.log 2>&1 &
API_PID=$!
./target/debug/signaling --bind "127.0.0.1:${SIG_PORT}" > /tmp/dual-android-sig.log 2>&1 &
SIG_PID=$!
cleanup() { kill $API_PID $SIG_PID 2>/dev/null || true; }
trap cleanup EXIT

api_up=0
for i in $(seq 1 50); do
  if curl -sf --max-time 1 "$API/healthz" >/dev/null; then api_up=1; break; fi
  if ! kill -0 $API_PID 2>/dev/null; then
    echo "device-api died — see /tmp/dual-android-api.log"
    cat /tmp/dual-android-api.log || true
    exit 1
  fi
  sleep 0.1
done
[ "$api_up" = "1" ] || { cat /tmp/dual-android-api.log; exit 1; }
echo "api ok pid=$API_PID"

EA=$(./target/debug/steward --api "$API" enroll)
EB=$(./target/debug/steward --api "$API" enroll)
DOG_A=$(echo "$EA" | sed -n 's/^dog_id=//p')
TERM_A=$(echo "$EA" | sed -n 's/^terminal_id=//p')
TOKEN_A=$(echo "$EA" | sed -n 's/^token=//p')
DOG_B=$(echo "$EB" | sed -n 's/^dog_id=//p')
TERM_B=$(echo "$EB" | sed -n 's/^terminal_id=//p')
TOKEN_B=$(echo "$EB" | sed -n 's/^token=//p')
./target/debug/steward --api "$API" bond --dog-a "$DOG_A" --dog-b "$DOG_B"

grant_pkg() {
  local serial=$1 pkg=$2
  adb -s "$serial" shell pm grant "$pkg" android.permission.CAMERA 2>/dev/null || true
  adb -s "$serial" shell pm grant "$pkg" android.permission.RECORD_AUDIO 2>/dev/null || true
}

echo "==> Install packages + reverse"
if [ "${SKIP_INSTALL:-0}" != "1" ]; then
  timeout 90 adb -s "$SERIAL_A" install -r -g "$APK_A"
  grant_pkg "$SERIAL_A" "$PKG_A"
  if [ "$MODE" = "dual_avd" ]; then
    timeout 90 adb -s "$SERIAL_B" install -r -g "$APK_B"
    grant_pkg "$SERIAL_B" "$PKG_B"
  else
    timeout 90 adb -s "$SERIAL_A" install -r -g "$APK_B"
    grant_pkg "$SERIAL_A" "$PKG_B"
  fi
else
  grant_pkg "$SERIAL_A" "$PKG_A"
  grant_pkg "$SERIAL_B" "$PKG_B"
fi

# reverse once per physical device
adb -s "$SERIAL_A" reverse tcp:${API_PORT} tcp:${API_PORT} || true
adb -s "$SERIAL_A" reverse tcp:${SIG_PORT} tcp:${SIG_PORT} || true
if [ "$SERIAL_A" != "$SERIAL_B" ]; then
  adb -s "$SERIAL_B" reverse tcp:${API_PORT} tcp:${API_PORT} || true
  adb -s "$SERIAL_B" reverse tcp:${SIG_PORT} tcp:${SIG_PORT} || true
fi
echo "reverse list A:"; adb -s "$SERIAL_A" reverse --list || true

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
ROLE_B=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['webrtc_role'])" "$ACC")
ROLE_A=offerer
echo "==> Session $SESS A=$ROLE_A B=$ROLE_B pkgs $PKG_A / $PKG_B"

PORTAL_A="http://127.0.0.1:${API_PORT}/portal/?api=http://127.0.0.1:${API_PORT}&signal=ws://127.0.0.1:${SIG_PORT}&token=${TOKEN_A}&terminalId=${TERM_A}&dogId=${DOG_A}&session=${SESS}&role=${ROLE_A}&autostart=1"
PORTAL_B="http://127.0.0.1:${API_PORT}/portal/?api=http://127.0.0.1:${API_PORT}&signal=ws://127.0.0.1:${SIG_PORT}&token=${TOKEN_B}&terminalId=${TERM_B}&dogId=${DOG_B}&session=${SESS}&role=${ROLE_B}&autostart=1"

adb -s "$SERIAL_A" logcat -c || true
[ "$SERIAL_A" != "$SERIAL_B" ] && adb -s "$SERIAL_B" logcat -c || true
adb -s "$SERIAL_A" shell am force-stop "$PKG_A" 2>/dev/null || true
adb -s "$SERIAL_B" shell am force-stop "$PKG_B" 2>/dev/null || true

# Launch B (answerer) first, then A (offerer)
adb -s "$SERIAL_B" shell am start -n "${PKG_B}/.MainActivity" --es portal_url "'${PORTAL_B}'"
sleep 4
adb -s "$SERIAL_A" shell am start -n "${PKG_A}/.MainActivity" --es portal_url "'${PORTAL_A}'"
echo "portals launched; waiting for WebRTC..."
sleep 18

curl -sf --max-time 3 -X POST "$API/v1/sessions/${SESS}/media_ready" \
  -H "Authorization: Device ${TERM_A}:${TOKEN_A}" -H 'Content-Type: application/json' \
  -d "{\"dog_id\":\"$DOG_A\",\"terminal_id\":\"$TERM_A\",\"ready\":true}" >/dev/null || true
MR=$(curl -sf --max-time 3 -X POST "$API/v1/sessions/${SESS}/media_ready" \
  -H "Authorization: Device ${TERM_B}:${TOKEN_B}" -H 'Content-Type: application/json' \
  -d "{\"dog_id\":\"$DOG_B\",\"terminal_id\":\"$TERM_B\",\"ready\":true}" || echo '{}')
BOTH=$(python3 -c "import json,sys; print(json.loads(sys.argv[1] or '{}').get('both_ready'))" "$MR" 2>/dev/null || echo False)
ACTIVE=$(curl -sf --max-time 3 "$API/v1/sessions/active?dog_id=${DOG_A}&terminal_id=${TERM_A}" \
  -H "Authorization: Device ${TERM_A}:${TOKEN_A}" || echo '{}')

# Combined logcat on same device; filter by role markers in portal logs
LOG_ALL=$(timeout 10 adb -s "$SERIAL_A" logcat -d 2>/dev/null | grep -E 'CanisLink|MEDIA_PATH|signal open|autostart|getUserMedia|portal:' | tail -200 || true)
if [ "$SERIAL_A" != "$SERIAL_B" ]; then
  LOG_B_RAW=$(timeout 10 adb -s "$SERIAL_B" logcat -d 2>/dev/null | grep -E 'CanisLink|MEDIA_PATH|signal open|autostart|getUserMedia|portal:' | tail -100 || true)
else
  LOG_B_RAW="$LOG_ALL"
fi
LOG_A="$LOG_ALL"
LOG_B="$LOG_B_RAW"

mkdir -p docs/lab
timeout 15 adb -s "$SERIAL_A" exec-out screencap -p > docs/lab/dual-android-a.png 2>/dev/null || true
# same device screenshot for B slot if single
cp docs/lab/dual-android-a.png docs/lab/dual-android-b.png 2>/dev/null || true
if [ "$SERIAL_A" != "$SERIAL_B" ]; then
  timeout 15 adb -s "$SERIAL_B" exec-out screencap -p > docs/lab/dual-android-b.png 2>/dev/null || true
fi

echo "--- log snippet ---"; echo "$LOG_ALL" | tail -40
echo "$LOG_ALL" > /tmp/dual_log_all.txt
echo "$ACTIVE" > /tmp/dual_active.json
echo "$MR" > /tmp/dual_mr.json

python3 - << PY
import json, re
log = open("/tmp/dual_log_all.txt").read()
active = json.loads(open("/tmp/dual_active.json").read() or "{}")
mr = json.loads(open("/tmp/dual_mr.json").read() or "{}")

def hits(pat):
    return len(re.findall(pat, log, re.I))

report = {
  "mode": "$MODE",
  "ok": bool(active and active.get("id") == "$SESS"),
  "session_id": "$SESS",
  "invite_id": "$INV_ID",
  "dog_a": "$DOG_A",
  "dog_b": "$DOG_B",
  "serial_a": "$SERIAL_A",
  "serial_b": "$SERIAL_B",
  "pkg_a": "$PKG_A",
  "pkg_b": "$PKG_B",
  "role_a": "$ROLE_A",
  "role_b": "$ROLE_B",
  "both_media_ready": bool(mr.get("both_ready")),
  "session_state": active.get("state") if active else None,
  "media_getUserMedia_hits": hits(r"MEDIA_PATH=getUserMedia"),
  "media_lab_canvas_hits": hits(r"MEDIA_PATH=lab_canvas"),
  "signal_open_hits": hits(r"signal open"),
  "autostart_hits": hits(r"autostart WebRTC"),
  "offer_hits": hits(r"sent offer"),
  "answer_hits": hits(r"sent answer"),
  "remote_track_hits": hits(r"remote track"),
  "pc_connected_hits": hits(r"pc connected|connectionstate.*connected"),
  "screenshot_a": "docs/lab/dual-android-a.png",
  "screenshot_b": "docs/lab/dual-android-b.png",
  "note": "lab dual Android portal peers; same_device uses peer1+peer2 flavors",
}
# Dual success: active session + both roles autostarted + media path + signaling
has_media = report["media_getUserMedia_hits"] > 0 or report["media_lab_canvas_hits"] > 0
report["dual_peer_ok"] = (
    report["ok"]
    and has_media
    and report["signal_open_hits"] >= 1
    and report["autostart_hits"] >= 2
)
open("$REPORT", "w").write(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
if not report["dual_peer_ok"]:
    raise SystemExit("DUAL_ANDROID_E2E incomplete")
print("DUAL_ANDROID_E2E_OK")
PY
