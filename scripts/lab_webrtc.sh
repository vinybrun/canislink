#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
cargo build -q -p signaling -p canis-media
./target/debug/signaling --bind 127.0.0.1:8081 &
SIG=$!
trap 'kill $SIG 2>/dev/null || true' EXIT
sleep 0.3
SESSION=$(cat /proc/sys/kernel/random/uuid 2>/dev/null || python3 -c 'import uuid;print(uuid.uuid4())')
DOG_A=$(python3 -c 'import uuid;print(uuid.uuid4())')
DOG_B=$(python3 -c 'import uuid;print(uuid.uuid4())')
echo "session=$SESSION"
./target/debug/canis-media --signal ws://127.0.0.1:8081 --session "$SESSION" --dog "$DOG_B" --role answerer &
ANS=$!
sleep 0.5
./target/debug/canis-media --signal ws://127.0.0.1:8081 --session "$SESSION" --dog "$DOG_A" --role offerer
kill $ANS 2>/dev/null || true
wait $ANS 2>/dev/null || true
echo "lab_webrtc OK"
