#!/usr/bin/env bash
set -euo pipefail

# Generate Swift and Kotlin from the same native library used for the mobile bindings.
cd "$(dirname "${BASH_SOURCE[0]}")/.."
swift_dir=apps/ios/Generated
kotlin_dir=apps/android/app/src/main/java
out_swift=$swift_dir
out_kotlin=$kotlin_dir
if [[ $# == 1 && $1 == --check ]]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  out_swift="$tmp/swift"
  out_kotlin="$tmp/kotlin"
elif [[ $# != 0 ]]; then
  echo "Usage: $0 [--check]" >&2
  exit 2
fi

cargo build -p orbit-ffi-uniffi
ext=so
if [[ $(uname -s) == Darwin ]]; then
  ext=dylib
fi
library="${CARGO_TARGET_DIR:-target}/debug/liborbit_ffi_uniffi.$ext"
mkdir -p "$out_swift" "$out_kotlin"
cargo run -p orbit-uniffi-bindgen --bin uniffi-bindgen -- generate \
  --library "$library" --language swift --out-dir "$out_swift"
cargo run -p orbit-uniffi-bindgen --bin uniffi-bindgen -- generate \
  --library "$library" --language kotlin --no-format --out-dir "$out_kotlin"

if [[ $# == 1 ]]; then
  status=0
  diff -rq "$swift_dir" "$out_swift" || status=1
  diff -rq "$kotlin_dir/uniffi" "$out_kotlin/uniffi" || status=1
  exit "$status"
fi
echo "Synchronized Swift ($swift_dir) and Kotlin ($kotlin_dir/uniffi/orbit_ffi_uniffi) bindings."
