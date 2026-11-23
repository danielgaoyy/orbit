#!/usr/bin/env bash
set -euo pipefail

# Build `OrbitCoreFFI.xcframework` (iOS 16.4+ Device & optional Simulator) and refresh Swift UniFFI bindings
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$IOS_DIR/../.." && pwd)"

DEVICE_ONLY=0
if [ "${ORBIT_IOS_DEVICE_ONLY:-0}" = "1" ]; then
  DEVICE_ONLY=1
fi

for arg in "$@"; do
  case "$arg" in
    --device-only)
      DEVICE_ONLY=1
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      echo "Usage: $0 [--device-only]" >&2
      exit 1
      ;;
  esac
done

export IPHONEOS_DEPLOYMENT_TARGET="16.4"

if [ "$DEVICE_ONLY" = "1" ]; then
  rustup target add aarch64-apple-ios
else
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim
fi

cd "$REPO_ROOT"
cargo build --release -p orbit-ffi-uniffi --target aarch64-apple-ios
if [ "$DEVICE_ONLY" = "0" ]; then
  cargo build --release -p orbit-ffi-uniffi --target aarch64-apple-ios-sim
fi

mkdir -p "$IOS_DIR/Generated"
cargo run -p orbit-uniffi-bindgen --bin uniffi-bindgen -- generate \
  --library "$REPO_ROOT/target/aarch64-apple-ios/release/liborbit_ffi_uniffi.a" \
  --language swift \
  --out-dir "$IOS_DIR/Generated"

HEADERS_STAGE="$IOS_DIR/build/headers"
rm -rf "$HEADERS_STAGE" \
  "$IOS_DIR/Frameworks/OrbitCoreFFI.xcframework" \
  "$IOS_DIR/build/SimDerivedData/Build/Products" \
  "$IOS_DIR/build/ReleaseDerivedData/Build/Products"
mkdir -p "$HEADERS_STAGE" "$IOS_DIR/Frameworks"

cp "$IOS_DIR/Generated/orbit_ffi_uniffiFFI.h" "$HEADERS_STAGE/"
cp "$IOS_DIR/Generated/orbit_ffi_uniffiFFI.modulemap" "$HEADERS_STAGE/module.modulemap"

XCFRAMEWORK_ARGS=(
  -library "$REPO_ROOT/target/aarch64-apple-ios/release/liborbit_ffi_uniffi.a"
  -headers "$HEADERS_STAGE"
)
if [ "$DEVICE_ONLY" = "0" ]; then
  XCFRAMEWORK_ARGS+=(
    -library "$REPO_ROOT/target/aarch64-apple-ios-sim/release/liborbit_ffi_uniffi.a"
    -headers "$HEADERS_STAGE"
  )
fi

xcodebuild -create-xcframework \
  "${XCFRAMEWORK_ARGS[@]}" \
  -output "$IOS_DIR/Frameworks/OrbitCoreFFI.xcframework"

if [ "$DEVICE_ONLY" = "1" ]; then
  echo "Built OrbitCoreFFI.xcframework (iOS 16.4+ device-only)"
else
  echo "Built OrbitCoreFFI.xcframework (iOS 16.4+ aligned with Paseo)"
fi
