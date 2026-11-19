#!/usr/bin/env bash
set -euo pipefail

# Build `OrbitCoreFFI.xcframework` (iOS 16.4+ Device & Simulator) and refresh Swift UniFFI bindings
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$IOS_DIR/../.." && pwd)"

export IPHONEOS_DEPLOYMENT_TARGET="16.4"

rustup target add aarch64-apple-ios aarch64-apple-ios-sim

cd "$REPO_ROOT"
cargo build --release -p orbit-ffi-uniffi --target aarch64-apple-ios
cargo build --release -p orbit-ffi-uniffi --target aarch64-apple-ios-sim
cargo build -p orbit-ffi-uniffi

mkdir -p "$IOS_DIR/Generated"
cargo run -p orbit-ffi-uniffi --bin uniffi-bindgen -- generate \
  --library "$REPO_ROOT/target/debug/liborbit_ffi_uniffi.dylib" \
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

xcodebuild -create-xcframework \
  -library "$REPO_ROOT/target/aarch64-apple-ios/release/liborbit_ffi_uniffi.a" \
  -headers "$HEADERS_STAGE" \
  -library "$REPO_ROOT/target/aarch64-apple-ios-sim/release/liborbit_ffi_uniffi.a" \
  -headers "$HEADERS_STAGE" \
  -output "$IOS_DIR/Frameworks/OrbitCoreFFI.xcframework"

echo "Built OrbitCoreFFI.xcframework (iOS 16.4+ aligned with Paseo)"
