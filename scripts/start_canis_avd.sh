#!/usr/bin/env bash
# Start CanisLab AVD with virtual cameras (no host webcam required).
set -euo pipefail
export ANDROID_HOME=${ANDROID_HOME:-/opt/android-sdk}
export ANDROID_SDK_ROOT=$ANDROID_HOME
export PATH=$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator
export LD_LIBRARY_PATH=$ANDROID_HOME/emulator/lib64:$ANDROID_HOME/emulator/lib64/qt/lib:${LD_LIBRARY_PATH:-}

AVD=${1:-CanisLab}
# second instance: pass CanisLab2 and -port 5556 via EXTRA
EXTRA=("${@:2}")

if adb devices 2>/dev/null | grep -qE "emulator-.*[[:space:]]device"; then
  if [[ "${FORCE_NEW:-}" != "1" ]]; then
    echo "emulator already running:"
    adb devices -l
    exit 0
  fi
fi

export QT_QPA_PLATFORM=${QT_QPA_PLATFORM:-offscreen}
export LD_LIBRARY_PATH=$ANDROID_HOME/emulator/lib64:$ANDROID_HOME/emulator/lib64/qt/lib:${LD_LIBRARY_PATH:-}

PORT_ARG=()
# second peer: ./scripts/start_canis_avd.sh CanisLab2 5556
if [[ "${2:-}" =~ ^[0-9]+$ ]]; then
  PORT_ARG=(-port "$2")
  EXTRA=("${@:3}")
else
  EXTRA=("${@:2}")
fi

echo "starting AVD=$AVD camera=virtualscene/emulated ${PORT_ARG[*]:-} ${EXTRA[*]:-}"
# headless lab: -no-window avoids Qt xcb requirement; virtualscene = synthetic camera
nohup emulator -avd "$AVD" \
  "${PORT_ARG[@]}" \
  -no-window -no-audio -no-boot-anim -no-metrics \
  -camera-back virtualscene \
  -camera-front emulated \
  -no-snapshot-save \
  -gpu swiftshader_indirect \
  -memory "${EMU_MEMORY:-2048}" \
  "${EXTRA[@]}" \
  >"/tmp/emulator-${AVD}.log" 2>&1 &
echo "emulator pid $! log /tmp/emulator-${AVD}.log"
echo "wait: adb wait-for-device && adb shell getprop sys.boot_completed"
