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
  node - "$swift_dir" "$out_swift" "$kotlin_dir/uniffi" "$out_kotlin/uniffi" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');

const pairs = [
  [process.argv[2], process.argv[3]],
  [process.argv[4], process.argv[5]],
];

function listFiles(root) {
  const out = new Map();
  if (!fs.existsSync(root)) {
    return out;
  }
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      const rel = path.relative(root, full);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.isFile()) {
        out.set(rel, full);
      }
    }
  };
  walk(root);
  return out;
}

let status = 0;
for (const [expectedDir, actualDir] of pairs) {
  const expectedFiles = listFiles(expectedDir);
  const actualFiles = listFiles(actualDir);
  const allRel = new Set([...expectedFiles.keys(), ...actualFiles.keys()]);
  for (const rel of [...allRel].sort()) {
    const expPath = expectedFiles.get(rel);
    const actPath = actualFiles.get(rel);
    if (!expPath) {
      console.error(`Only in generated (${actualDir}): ${rel}`);
      status = 1;
    } else if (!actPath) {
      console.error(`Only in checked-in (${expectedDir}): ${rel}`);
      status = 1;
    } else {
      const expBuf = fs.readFileSync(expPath);
      const actBuf = fs.readFileSync(actPath);
      if (Buffer.compare(expBuf, actBuf) !== 0) {
        console.error(`Files differ: ${expPath} vs ${actPath}`);
        status = 1;
      }
    }
  }
}
process.exit(status);
NODE
  exit $?
fi
echo "Synchronized Swift ($swift_dir) and Kotlin ($kotlin_dir/uniffi/orbit_ffi_uniffi) bindings."
