#!/usr/bin/env bash
set -euo pipefail

# Build `liborbit_ohos.so` for HarmonyOS NEXT 7.0+ (API 26, arm64-v8a & x86_64)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

if [ -z "${OHOS_NDK_HOME:-}" ] && [ -d "/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony/native" ]; then
  export OHOS_NDK_HOME="/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony/native"
fi

if [ -z "${OHOS_NDK_HOME:-}" ] || [ ! -d "$OHOS_NDK_HOME" ]; then
  echo "Error: OHOS_NDK_HOME not found." >&2
  exit 1
fi

rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos

LLVM_BIN="$OHOS_NDK_HOME/llvm/bin"
SYSROOT="$OHOS_NDK_HOME/sysroot"

export CC_aarch64_unknown_linux_ohos="$LLVM_BIN/aarch64-unknown-linux-ohos-clang"
export CXX_aarch64_unknown_linux_ohos="$LLVM_BIN/aarch64-unknown-linux-ohos-clang++"
export AR_aarch64_unknown_linux_ohos="$LLVM_BIN/llvm-ar"
export CFLAGS_aarch64_unknown_linux_ohos="--sysroot=$SYSROOT"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$LLVM_BIN/aarch64-unknown-linux-ohos-clang"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="-C link-arg=--sysroot=$SYSROOT -C link-arg=-Wl,-z,max-page-size=16384"

export CC_x86_64_unknown_linux_ohos="$LLVM_BIN/x86_64-unknown-linux-ohos-clang"
export CXX_x86_64_unknown_linux_ohos="$LLVM_BIN/x86_64-unknown-linux-ohos-clang++"
export AR_x86_64_unknown_linux_ohos="$LLVM_BIN/llvm-ar"
export CFLAGS_x86_64_unknown_linux_ohos="--sysroot=$SYSROOT"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_LINKER="$LLVM_BIN/x86_64-unknown-linux-ohos-clang"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="-C link-arg=--sysroot=$SYSROOT -C link-arg=-Wl,-z,max-page-size=16384"

cd "$REPO_ROOT"
cargo build --release -p orbit-ffi-ohos --target aarch64-unknown-linux-ohos
cargo build --release -p orbit-ffi-ohos --target x86_64-unknown-linux-ohos

mkdir -p "$REPO_ROOT/apps/harmony/entry/libs/arm64-v8a"
mkdir -p "$REPO_ROOT/apps/harmony/entry/libs/x86_64"

cp -f "$REPO_ROOT/target/aarch64-unknown-linux-ohos/release/liborbit_ohos.so" \
  "$REPO_ROOT/apps/harmony/entry/libs/arm64-v8a/"
cp -f "$REPO_ROOT/target/x86_64-unknown-linux-ohos/release/liborbit_ohos.so" \
  "$REPO_ROOT/apps/harmony/entry/libs/x86_64/"

echo "Built HarmonyOS NEXT 7.0+ native N-API libraries (arm64-v8a + x86_64) into apps/harmony/entry/libs/"
