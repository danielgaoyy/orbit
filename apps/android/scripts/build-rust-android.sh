#!/usr/bin/env bash
set -euo pipefail

# Build `liborbit_ffi_uniffi.so` for Android (minSdk 29 / ARM64 & x86_64) and regenerate Kotlin UniFFI bindings
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$ANDROID_DIR/../.." && pwd)"

rustup target add aarch64-linux-android x86_64-linux-android

if ! command -v cargo-ndk >/dev/null 2>&1; then
  cargo install cargo-ndk
fi

cd "$REPO_ROOT"
cargo ndk \
  -t arm64-v8a \
  -t x86_64 \
  --platform 29 \
  -o "$ANDROID_DIR/app/src/main/jniLibs" \
  build --release -p orbit-ffi-uniffi

cargo build -p orbit-ffi-uniffi
DYN_EXT="so"
if [ "$(uname -s)" = "Darwin" ]; then
  DYN_EXT="dylib"
fi

cargo run -p orbit-ffi-uniffi --bin uniffi-bindgen -- generate \
  --library "$REPO_ROOT/target/debug/liborbit_ffi_uniffi.$DYN_EXT" \
  --language kotlin \
  --no-format \
  --out-dir "$ANDROID_DIR/app/src/main/java"

echo "Built Android JNI libraries (API 29+) and Kotlin UniFFI bindings."
