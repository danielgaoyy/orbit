#!/usr/bin/env bash
set -euo pipefail

# Regenerate Swift (iOS) and Kotlin (Android) UniFFI bindings from `crates/orbit-ffi-uniffi`.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"
cargo build -p orbit-ffi-uniffi

DYN_EXT="so"
if [ "$(uname -s)" = "Darwin" ]; then
  DYN_EXT="dylib"
fi

LIB_PATH="$REPO_ROOT/target/debug/liborbit_ffi_uniffi.$DYN_EXT"
if [ ! -f "$LIB_PATH" ]; then
  echo "error: expected built cdylib at $LIB_PATH" >&2
  exit 1
fi

mkdir -p "$REPO_ROOT/apps/ios/Generated"
cargo run -p orbit-ffi-uniffi --bin uniffi-bindgen -- generate \
  --library "$LIB_PATH" \
  --language swift \
  --out-dir "$REPO_ROOT/apps/ios/Generated"

mkdir -p "$REPO_ROOT/apps/android/app/src/main/java"
cargo run -p orbit-ffi-uniffi --bin uniffi-bindgen -- generate \
  --library "$LIB_PATH" \
  --language kotlin \
  --no-format \
  --out-dir "$REPO_ROOT/apps/android/app/src/main/java"

echo "Synchronized Swift (apps/ios/Generated) and Kotlin (apps/android/app/src/main/java/uniffi/orbit_ffi_uniffi) bindings."
