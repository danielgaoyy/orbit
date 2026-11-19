#!/usr/bin/env bash
set -euo pipefail

# Build `liborbit_ffi_uniffi.so` for Android (minSdk 29 / targetSdk 36, 16KB page-aligned) & refresh Kotlin UniFFI bindings
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$ANDROID_DIR/../.." && pwd)"

rustup target add aarch64-linux-android x86_64-linux-android

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  for candidate in \
    /opt/homebrew/share/android-commandlinetools/ndk/* \
    "${ANDROID_HOME:-}/ndk/"* \
    "$HOME/Library/Android/sdk/ndk/"*; do
    if [ -d "$candidate" ]; then
      export ANDROID_NDK_HOME="$candidate"
    fi
  done
fi

if [ -z "${ANDROID_NDK_HOME:-}" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
  echo "Error: ANDROID_NDK_HOME not found." >&2
  exit 1
fi

LLVM_BIN="$(ls -d "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin | head -n 1)"

export CC_aarch64_linux_android="$LLVM_BIN/aarch64-linux-android29-clang"
export CXX_aarch64_linux_android="$LLVM_BIN/aarch64-linux-android29-clang++"
export AR_aarch64_linux_android="$LLVM_BIN/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LLVM_BIN/aarch64-linux-android29-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"

export CC_x86_64_linux_android="$LLVM_BIN/x86_64-linux-android29-clang"
export CXX_x86_64_linux_android="$LLVM_BIN/x86_64-linux-android29-clang++"
export AR_x86_64_linux_android="$LLVM_BIN/llvm-ar"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$LLVM_BIN/x86_64-linux-android29-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"

cd "$REPO_ROOT"
cargo build --release -p orbit-ffi-uniffi --target aarch64-linux-android
cargo build --release -p orbit-ffi-uniffi --target x86_64-linux-android

mkdir -p "$ANDROID_DIR/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$ANDROID_DIR/app/src/main/jniLibs/x86_64"
cp -f "$REPO_ROOT/target/aarch64-linux-android/release/liborbit_ffi_uniffi.so" \
  "$ANDROID_DIR/app/src/main/jniLibs/arm64-v8a/"
cp -f "$REPO_ROOT/target/x86_64-linux-android/release/liborbit_ffi_uniffi.so" \
  "$ANDROID_DIR/app/src/main/jniLibs/x86_64/"

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

echo "Built 16KB-aligned Android JNI libraries (arm64-v8a + x86_64) and Kotlin UniFFI bindings."
