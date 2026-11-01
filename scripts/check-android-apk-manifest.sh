#!/usr/bin/env bash
# Issue #888: checks that the merged manifest of a built APK still carries the
# permissions, the microphone feature and the foreground SessionService that
# `tauri-plugin-android-host` contributes through manifest merge.
# Usage: scripts/check-android-apk-manifest.sh <apk>
set -euo pipefail

apk="${1:?usage: $0 <apk>}"
[ -f "$apk" ] || { echo "::error::APK not found: $apk"; exit 1; }

aapt="${AAPT:-}"
if [ -z "$aapt" ]; then
  aapt="$(find "${ANDROID_HOME:-/nonexistent}/build-tools" -name aapt -type f 2>/dev/null | sort -V | tail -n1 || true)"
fi
[ -n "$aapt" ] && [ -x "$aapt" ] || { echo "::error::aapt not found (set AAPT or ANDROID_HOME)"; exit 1; }

dump="$("$aapt" dump xmltree "$apk" AndroidManifest.xml)"
fail=0
need() {
  if ! grep -qF -- "$1" <<<"$dump"; then
    echo "::error::merged manifest of $(basename "$apk") lacks: $1"
    fail=1
  fi
}

for perm in RECORD_AUDIO FOREGROUND_SERVICE FOREGROUND_SERVICE_SPECIAL_USE POST_NOTIFICATIONS WAKE_LOCK; do
  need "android.permission.$perm\""
done
need 'android.hardware.microphone'
need 'dev.smartssh.androidhost.SessionService'

# The service element must carry foregroundServiceType=specialUse (0x40000000).
if ! awk '
  /E: service / { in_service = 1; next }
  in_service && /E: / { in_service = 0 }
  in_service && /foregroundServiceType/ && /0x40000000/ { found = 1 }
  END { exit !found }
' <<<"$dump"; then
  echo "::error::SessionService has no foregroundServiceType=specialUse"
  fail=1
fi

[ "$fail" -eq 0 ] && echo "merged manifest of $(basename "$apk") is complete"
exit "$fail"
