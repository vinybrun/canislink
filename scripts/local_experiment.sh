#!/usr/bin/env bash
# Deploy local CanisLink experiment with phone + embedded emulators and run e2e.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

API_PORT=${API_PORT:-18080}
SIG_PORT=${SIG_PORT:-18081}
API="http://127.0.0.1:${API_PORT}"
SIGNAL="ws://127.0.0.1:${SIG_PORT}"
DB="${TMPDIR:-/tmp}/canis-lab-experiment.db"
REPORT="${REPORT:-docs/lab/experiment-report.json}"
STEWARD_SECRET="${CANIS_STEWARD_SECRET:-canis-steward-secret}"
DEVICE_SECRET="${CANIS_DEVICE_SECRET:-canis-dev-secret}"

echo "==> Building experiment stack"
cargo build -q -p device-api -p signaling -p steward -p sim-dog -p canis-media -p lab-experiment

echo "==> Starting device-api (SQLite durable) on :${API_PORT}"
rm -f "$DB"
./target/debug/device-api \
  --bind "127.0.0.1:${API_PORT}" \
  --database-url "sqlite:${DB}?mode=rwc" \
  --device-secret "$DEVICE_SECRET" \
  --steward-secret "$STEWARD_SECRET" \
  > /tmp/canis-exp-api.log 2>&1 &
API_PID=$!

echo "==> Starting signaling on :${SIG_PORT}"
./target/debug/signaling --bind "127.0.0.1:${SIG_PORT}" \
  > /tmp/canis-exp-sig.log 2>&1 &
SIG_PID=$!

cleanup() {
  kill "$API_PID" "$SIG_PID" 2>/dev/null || true
  wait "$API_PID" 2>/dev/null || true
  wait "$SIG_PID" 2>/dev/null || true
}
trap cleanup EXIT

echo "==> Waiting for health"
for i in $(seq 1 50); do
  if curl -sf "$API/healthz" >/dev/null && curl -sf "http://127.0.0.1:${SIG_PORT}/healthz" >/dev/null; then
    break
  fi
  sleep 0.1
done
curl -sf "$API/healthz" | head -c 200; echo
curl -sf "http://127.0.0.1:${SIG_PORT}/healthz" | head -c 200; echo

echo "==> Running lab-experiment (phones + embedded + WebRTC)"
./target/debug/lab-experiment \
  --api "$API" \
  --signal "$SIGNAL" \
  --steward-secret "$STEWARD_SECRET" \
  --bin-dir target/debug \
  --report "$REPORT" \
  --with-webrtc

echo "==> Experiment complete"
echo "    API log:  /tmp/canis-exp-api.log"
echo "    SIG log:  /tmp/canis-exp-sig.log"
echo "    DB:       $DB"
echo "    Report:   $REPORT"
