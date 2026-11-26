#!/usr/bin/env bash
set -euo pipefail

# HarmonyOS PR-1 stream replay performance harness (§8.1).
# Builds (unless --skip-build) the `perf-hooks` liborbit_ohos.so + debug HAP,
# pushes `stream-500.json` to an online HarmonyOS device/emulator, cold-starts
# EntryAbility into replay mode while capturing hitrace (`graphic ace app`),
# and pulls both `.ftrace` and filtered `OrbitPerf` hilog into
# `apps/harmony/build/perf_artifacts/`.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARMONY_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPO_ROOT="$(cd "$HARMONY_DIR/../.." && pwd)"

DEVECO_ROOT="/Applications/DevEco-Studio.app/Contents"
DEVECO_SDK_ROOT="${DEVECO_SDK_HOME:-$DEVECO_ROOT/sdk}"
export DEVECO_SDK_HOME="$DEVECO_SDK_ROOT"
export OHOS_NDK_HOME="${OHOS_NDK_HOME:-$DEVECO_SDK_ROOT/default/openharmony}"
export JAVA_HOME="${JAVA_HOME:-$DEVECO_ROOT/jbr/Contents/Home}"

NODE_BIN="$DEVECO_ROOT/tools/node/bin/node"
OHPM_BIN="$DEVECO_ROOT/tools/ohpm/bin/ohpm"
HVIGORW="$DEVECO_ROOT/tools/hvigor/bin/hvigorw.js"
HDC="$DEVECO_SDK_ROOT/default/openharmony/toolchains/hdc"

export PATH="$JAVA_HOME/bin:$DEVECO_ROOT/tools/ohpm/bin:$DEVECO_ROOT/tools/node/bin:$DEVECO_SDK_ROOT/default/openharmony/toolchains:$PATH"

BUNDLE_NAME="dev.orbit.harmony"
ABILITY_NAME="EntryAbility"
HAP_DIR="$HARMONY_DIR/entry/build/default/outputs/default"
UNSIGNED_HAP="$HAP_DIR/entry-default-unsigned.hap"
SIGNED_HAP="$HAP_DIR/entry-default-signed.hap"
FIXTURE_PATH="$SCRIPT_DIR/fixtures/stream-500.json"
OUT_DIR="$HARMONY_DIR/build/perf_artifacts"

SKIP_BUILD=0
RUN_REPORT=0
TARGET_ID=""
ARCH="auto"
TRACE_DURATION=20
REFRESH_HZ=0
REPLAY_HOST="replay-host"
REPLAY_AGENT="replay-agent"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --report) RUN_REPORT=1; shift ;;
    --target) TARGET_ID="${2:?--target requires a device/emulator id}"; shift 2 ;;
    --arch) ARCH="${2:?--arch requires auto|arm64|x86_64|both}"; shift 2 ;;
    --duration) TRACE_DURATION="${2:?--duration requires seconds}"; shift 2 ;;
    --refresh-hz) REFRESH_HZ="${2:?--refresh-hz requires 60 or 120}"; shift 2 ;;
    --fixture) FIXTURE_PATH="${2:?--fixture requires a file path}"; shift 2 ;;
    --out-dir) OUT_DIR="${2:?--out-dir requires a directory path}"; shift 2 ;;
    *)
      echo "Usage: $0 [--skip-build] [--report] [--target ID] [--arch auto|arm64|x86_64|both] [--duration SECS] [--refresh-hz 60|120] [--fixture PATH] [--out-dir DIR]" >&2
      exit 2
      ;;
  esac
done

if [ ! -x "$HDC" ]; then
  echo "Error: hdc executable not found at $HDC" >&2
  exit 1
fi
if [ ! -f "$FIXTURE_PATH" ]; then
  echo "Error: replay fixture not found at $FIXTURE_PATH" >&2
  exit 1
fi

# 1. Resolve online HarmonyOS target
if [ -z "$TARGET_ID" ]; then
  TARGETS_RAW="$("$HDC" list targets 2>&1 || true)"
  TARGET_ID="$(printf '%s\n' "$TARGETS_RAW" | /usr/bin/tr -d '\r' | /usr/bin/sed '/^[[:space:]]*$/d; /^\[Empty\]$/d; /Empty/d' | /usr/bin/head -n 1)"
fi
if [ -z "$TARGET_ID" ]; then
  echo "Error: no online HarmonyOS target found via $HDC list targets" >&2
  exit 1
fi

# 2. Detect device ABI when --arch auto
if [ "$ARCH" = "auto" ]; then
  ABI_LIST="$("$HDC" -t "$TARGET_ID" shell param get const.product.cpu.abilist 2>/dev/null | /usr/bin/tr -d '\r\n' || true)"
  case "$ABI_LIST" in
    *arm64-v8a*) ARCH="arm64" ;;
    *x86_64*) ARCH="x86_64" ;;
    *) ARCH="both" ;;
  esac
fi

# 3. Build perf-hooks .so and debug HAP unless --skip-build
if [ "$SKIP_BUILD" -eq 0 ]; then
  if [ ! -x "$NODE_BIN" ] || [ ! -x "$OHPM_BIN" ] || [ ! -f "$HVIGORW" ]; then
    echo "Error: DevEco toolchain binaries missing under $DEVECO_ROOT" >&2
    exit 1
  fi
  echo "Building liborbit_ohos.so with --features perf-hooks (--arch $ARCH)..."
  /bin/bash "$HARMONY_DIR/scripts/build-rust-ohos.sh" --arch "$ARCH" --features perf-hooks

  echo "Assembling entry@default debug HAP..."
  (
    cd "$HARMONY_DIR"
    "$OHPM_BIN" install
    "$NODE_BIN" "$HVIGORW" --mode module \
      -p module=entry@default -p product=default -p buildMode=debug assembleHap --no-daemon
  )
fi

HAP_PATH="$UNSIGNED_HAP"
if [ -f "$SIGNED_HAP" ] && { [ ! -f "$UNSIGNED_HAP" ] || [ "$SIGNED_HAP" -nt "$UNSIGNED_HAP" ]; }; then
  HAP_PATH="$SIGNED_HAP"
fi
if [ ! -f "$HAP_PATH" ]; then
  echo "Error: HAP not found at $HAP_PATH" >&2
  exit 1
fi

# 4. Prepare device state & install HAP
/bin/mkdir -p "$OUT_DIR"
echo "Target: $TARGET_ID (arch=$ARCH)"
echo "Installing HAP: $HAP_PATH"
"$HDC" -t "$TARGET_ID" shell aa force-stop "$BUNDLE_NAME" >/dev/null 2>&1 || true
"$HDC" -t "$TARGET_ID" install -r "$HAP_PATH"
"$HDC" -t "$TARGET_ID" shell hilog -r >/dev/null 2>&1 || true

# Detect refresh rate if not overridden
if [ "$REFRESH_HZ" -eq 0 ]; then
  REFRESH_RAW="$("$HDC" -t "$TARGET_ID" shell "hidumper -s DisplayManagerService -a '-a' 2>/dev/null | grep -iE 'refreshRate|fps' | head -n 1" 2>/dev/null | /usr/bin/tr -d '\r' || true)"
  if printf '%s' "$REFRESH_RAW" | /usr/bin/grep -q '120'; then
    REFRESH_HZ=120
  else
    REFRESH_HZ=60
  fi
fi

# 5. Push stream-500.json fixture to device and mirror into app sandbox files dir when accessible
REMOTE_TMP_FIXTURE="/data/local/tmp/orbit_stream_500.json"
REMOTE_SANDBOX_FILES="/data/app/el2/100/base/$BUNDLE_NAME/haps/entry/files"
REMOTE_SANDBOX_FIXTURE="$REMOTE_SANDBOX_FILES/orbit_stream_500.json"
REMOTE_TRACE="/data/local/tmp/orbit_stream_replay.ftrace"

"$HDC" -t "$TARGET_ID" file send "$FIXTURE_PATH" "$REMOTE_TMP_FIXTURE"
REPLAY_PARAM_PATH="$REMOTE_TMP_FIXTURE"
if "$HDC" -t "$TARGET_ID" shell "mkdir -p '$REMOTE_SANDBOX_FILES' && cp '$REMOTE_TMP_FIXTURE' '$REMOTE_SANDBOX_FIXTURE' && chmod 644 '$REMOTE_SANDBOX_FIXTURE'" >/dev/null 2>&1; then
  REPLAY_PARAM_PATH="files://orbit_stream_500.json"
fi

# 6. Capture hitrace while cold-starting EntryAbility in replay mode
"$HDC" -t "$TARGET_ID" shell rm -f "$REMOTE_TRACE" >/dev/null 2>&1 || true
echo "Starting hitrace (${TRACE_DURATION}s, graphic ace app)..."
"$HDC" -t "$TARGET_ID" shell "hitrace -t $TRACE_DURATION -b 16384 graphic ace app > '$REMOTE_TRACE'" &
TRACE_PID=$!

/bin/sleep 1
echo "Launching $BUNDLE_NAME/$ABILITY_NAME with replay fixture ($REPLAY_PARAM_PATH)..."
"$HDC" -t "$TARGET_ID" shell aa start \
  -a "$ABILITY_NAME" \
  -b "$BUNDLE_NAME" \
  --ps orbit_perf_replay_path "$REPLAY_PARAM_PATH" \
  --ps orbit_perf_replay_host "$REPLAY_HOST" \
  --ps orbit_perf_replay_agent "$REPLAY_AGENT"

wait "$TRACE_PID"

# 7. Pull trace and OrbitPerf hilog artifacts
LOCAL_TRACE="$OUT_DIR/stream-replay.ftrace"
LOCAL_HILOG_ALL="$OUT_DIR/hilog_full.txt"
LOCAL_HILOG_PERF="$OUT_DIR/orbit_perf_hilog.txt"
LOCAL_META="$OUT_DIR/run_meta.json"

"$HDC" -t "$TARGET_ID" file recv "$REMOTE_TRACE" "$LOCAL_TRACE"
"$HDC" -t "$TARGET_ID" shell hilog -x > "$LOCAL_HILOG_ALL"
/usr/bin/grep -E "OrbitPerf|JSCRASH|CPP_CRASH|Fatal" "$LOCAL_HILOG_ALL" > "$LOCAL_HILOG_PERF" || true

cat > "$LOCAL_META" <<EOF
{
  "targetId": "$TARGET_ID",
  "arch": "$ARCH",
  "refreshHz": $REFRESH_HZ,
  "traceDurationSec": $TRACE_DURATION,
  "fixturePath": "$FIXTURE_PATH",
  "replayParamPath": "$REPLAY_PARAM_PATH",
  "capturedAtIso": "$(/bin/date -u +"%Y-%m-%dT%H:%M:%SZ")"
}
EOF

echo "perf_artifacts: $OUT_DIR"
echo "  trace: $LOCAL_TRACE"
echo "  hilog: $LOCAL_HILOG_PERF"

if /usr/bin/grep -E "JSCRASH|CPP_CRASH|Fatal" "$LOCAL_HILOG_PERF" >/dev/null 2>&1; then
  echo "Error: crash or fatal log detected during replay in $LOCAL_HILOG_PERF" >&2
  /usr/bin/grep -E "JSCRASH|CPP_CRASH|Fatal" "$LOCAL_HILOG_PERF" >&2 || true
  exit 1
fi

if [ "$RUN_REPORT" -eq 1 ]; then
  if [ -x "$NODE_BIN" ]; then
    "$NODE_BIN" "$SCRIPT_DIR/report.mjs" --dir "$OUT_DIR" --refresh-hz "$REFRESH_HZ"
  else
    node "$SCRIPT_DIR/report.mjs" --dir "$OUT_DIR" --refresh-hz "$REFRESH_HZ"
  fi
fi
