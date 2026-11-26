#!/usr/bin/env bash
set -euo pipefail

# HarmonyOS PR-0 smoke verification script.
# Uses absolute tool paths derived from the standard DevEco Studio install
# without relying on PATH or introducing custom environment variables.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARMONY_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

DEVECO_ROOT="/Applications/DevEco-Studio.app/Contents"
DEVECO_SDK_ROOT="${DEVECO_SDK_HOME:-$DEVECO_ROOT/sdk}"
export JAVA_HOME="${JAVA_HOME:-$DEVECO_ROOT/jbr/Contents/Home}"
HDC="$DEVECO_SDK_ROOT/default/openharmony/toolchains/hdc"

BUNDLE_NAME="dev.orbit.harmony"
ABILITY_NAME="EntryAbility"
HAP_DIR="$HARMONY_DIR/entry/build/default/outputs/default"
UNSIGNED_HAP="$HAP_DIR/entry-default-unsigned.hap"
SIGNED_HAP="$HAP_DIR/entry-default-signed.hap"
ARTIFACTS_DIR="$HARMONY_DIR/build/smoke_artifacts"

REMOTE_SCREENSHOT="/data/local/tmp/orbit_smoke.jpeg"
REMOTE_LAYOUT="/data/local/tmp/orbit_layout.json"

if [ ! -x "$HDC" ]; then
  echo "Error: hdc executable not found at $HDC" >&2
  exit 1
fi

# 1. Verify at least one online target via hdc list targets
TARGETS_RAW="$("$HDC" list targets 2>&1 || true)"
TARGET_ID="$(printf '%s\n' "$TARGETS_RAW" | /usr/bin/tr -d '\r' | /usr/bin/sed '/^[[:space:]]*$/d; /^\[Empty\]$/d; /Empty/d' | /usr/bin/head -n 1)"
if [ -z "$TARGET_ID" ]; then
  echo "Error: no online HarmonyOS target found via $HDC list targets (output: $TARGETS_RAW)" >&2
  exit 1
fi

# 2. Install built HAP (prefer freshly built signed HAP when present for local dev signing)
HAP_PATH="$UNSIGNED_HAP"
if [ -f "$SIGNED_HAP" ] && { [ ! -f "$UNSIGNED_HAP" ] || [ "$SIGNED_HAP" -nt "$UNSIGNED_HAP" ]; }; then
  HAP_PATH="$SIGNED_HAP"
fi
if [ ! -f "$HAP_PATH" ]; then
  echo "Error: HAP not found at $HAP_PATH. Build entry@default first." >&2
  exit 1
fi

/bin/mkdir -p "$ARTIFACTS_DIR"
echo "Target: $TARGET_ID"
echo "Installing HAP: $HAP_PATH"
"$HDC" -t "$TARGET_ID" install -r "$HAP_PATH"

# 3. Start EntryAbility
"$HDC" -t "$TARGET_ID" shell aa start -a "$ABILITY_NAME" -b "$BUNDLE_NAME"

# 4. Wait 3s, capture display snapshot and receive artifact
/bin/sleep 3
"$HDC" -t "$TARGET_ID" shell snapshot_display -f "$REMOTE_SCREENSHOT"
"$HDC" -t "$TARGET_ID" file recv "$REMOTE_SCREENSHOT" "$ARTIFACTS_DIR/orbit_smoke.jpeg"

# 5. Dump UI layout tree and receive artifact
"$HDC" -t "$TARGET_ID" shell uitest dumpLayout -p "$REMOTE_LAYOUT"
"$HDC" -t "$TARGET_ID" file recv "$REMOTE_LAYOUT" "$ARTIFACTS_DIR/orbit_layout.json"

# 6. Collect recent hilog and scan for crashes/fatals/OrbitPerf markers
HILOG_ALL="$ARTIFACTS_DIR/hilog_recent.txt"
HILOG_FILTERED="$ARTIFACTS_DIR/hilog_filtered.txt"
"$HDC" -t "$TARGET_ID" shell hilog -x -n 500 > "$HILOG_ALL"
/usr/bin/grep -E "JSCRASH|CPP_CRASH|Fatal|OrbitPerf" "$HILOG_ALL" > "$HILOG_FILTERED" || true

if [ -s "$HILOG_FILTERED" ]; then
  /bin/cat "$HILOG_FILTERED"
fi

# 7. Report artifacts directory and fail if any crash/fatal was recorded
echo "smoke_artifacts: $ARTIFACTS_DIR"
if /usr/bin/grep -E "JSCRASH|CPP_CRASH|Fatal" "$HILOG_FILTERED" >/dev/null 2>&1; then
  echo "Error: detected crash or fatal log entry in $HILOG_FILTERED" >&2
  exit 1
fi
