#!/usr/bin/env bash
# Issue #797: installs the debug APK in the running emulator, starts the app,
# lets it run for a fixed time and fails when it crashed at launch.
# Usage: android-launch-smoke.sh <apk> <logcat-output-file>
set -u
APK="$1"
[ -d "$APK" ] && APK="$(find "$APK" -name "*.apk" | head -n1)"
LOG="$2"
PKG=com.smartssh.desktop
ACTIVITY="$PKG/.MainActivity"
UPTIME_SECONDS="${SMOKE_UPTIME_SECONDS:-30}"

adb install -r "$APK" || { echo "::error::APK install failed"; exit 1; }
adb logcat -c
adb shell am start -W -n "$ACTIVITY" || { echo "::error::activity start failed"; }

sleep "$UPTIME_SECONDS"

adb logcat -d > "$LOG"
fail=0

if [ -z "$(adb shell pidof "$PKG" | tr -d '\r')" ]; then
  echo "::error::app process is not running"; fail=1
fi
if grep -E 'panicked|FATAL EXCEPTION' "$LOG" > /dev/null; then
  echo "::error::logcat contains 'panicked' or 'FATAL EXCEPTION'"
  grep -E -m5 -A3 'panicked|FATAL EXCEPTION' "$LOG"
  fail=1
fi
if ! adb shell dumpsys activity activities | tr -d '\r' \
    | grep -E 'topResumedActivity|mResumedActivity|ResumedActivity' | grep -q "$PKG"; then
  echo "::error::$ACTIVITY is not in the foreground"; fail=1
fi

[ "$fail" -eq 0 ] && echo "Launch smoke test passed"
exit "$fail"
