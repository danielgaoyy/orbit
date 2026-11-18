#!/usr/bin/env bash
set -euo pipefail

# Build `liborbit_ohos.so` for HarmonyOS 7+ (API 26) using `ohrs` (`ohos-rs`)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

if ! command -v ohrs >/dev/null 2>&1; then
  echo "Installing ohrs (ohos-rs CLI)..."
  cargo install ohrs
fi

cd "$REPO_ROOT/crates/orbit-ffi-ohos"
ohrs build --release --arch arm64 --arch x64

mkdir -p "$REPO_ROOT/apps/harmony/entry/libs/arm64-v8a"
mkdir -p "$REPO_ROOT/apps/harmony/entry/libs/x86_64"

cp -f dist/arm64-v8a/liborbit_ohos.so "$REPO_ROOT/apps/harmony/entry/libs/arm64-v8a/"
if [ -f dist/x86_64/liborbit_ohos.so ]; then
  cp -f dist/x86_64/liborbit_ohos.so "$REPO_ROOT/apps/harmony/entry/libs/x86_64/"
fi

echo "HarmonyOS 7+ native Rust library copied to apps/harmony/entry/libs/"
