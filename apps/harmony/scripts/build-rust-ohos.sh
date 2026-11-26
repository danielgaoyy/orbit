#!/usr/bin/env bash
set -euo pipefail

# Build `liborbit_ohos.so` for HarmonyOS NEXT 7.0+ (API 26, arm64-v8a & x86_64)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

arch=both
features=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) arch="${2:?--arch requires arm64, x86_64 or both}"; shift 2 ;;
    --features) features="${2:?--features requires a Cargo feature list}"; shift 2 ;;
    *) echo "Usage: $0 [--arch arm64|x86_64|both] [--features perf-hooks]" >&2; exit 2 ;;
  esac
done
case "$arch" in
  arm64) targets=(aarch64) ;;
  x86_64) targets=(x86_64) ;;
  both) targets=(aarch64 x86_64) ;;
  *) echo "Unknown architecture: $arch" >&2; exit 2 ;;
esac

resolve_ohos_root() {
  local candidates=()
  if [ -n "${OHOS_NDK_HOME:-}" ]; then
    candidates+=("$OHOS_NDK_HOME")
  fi
  if [ -n "${HOS_SDK_HOME:-}" ]; then
    candidates+=("$HOS_SDK_HOME/default/openharmony" "$HOS_SDK_HOME/openharmony")
  fi
  if [ -n "${DEVECO_SDK_HOME:-}" ]; then
    candidates+=("$DEVECO_SDK_HOME/default/openharmony" "$DEVECO_SDK_HOME/openharmony")
  fi
  candidates+=("/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony")

  local candidate
  for candidate in "${candidates[@]}"; do
    if [ -d "$candidate/native/llvm/bin" ] && [ -d "$candidate/native/sysroot" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
    if [ -d "$candidate/llvm/bin" ] && [ -d "$candidate/sysroot" ]; then
      # Normalize `.../openharmony/native` -> `.../openharmony` so `napi-build-ohos`
      # resolves `{OHOS_NDK_HOME}/native/{sysroot,llvm}`.
      dirname "$candidate"
      return 0
    fi
  done
  return 1
}

if ! OHOS_ROOT="$(resolve_ohos_root)"; then
  echo "Error: HarmonyOS NDK not found. Set OHOS_NDK_HOME or HOS_SDK_HOME." >&2
  exit 1
fi

export OHOS_NDK_HOME="$OHOS_ROOT"
NATIVE_DIR="$OHOS_ROOT/native"
LLVM_BIN="$NATIVE_DIR/llvm/bin"
SYSROOT="$NATIVE_DIR/sysroot"


WRAPPER_DIR="$(mktemp -d)"
trap 'rm -rf "$WRAPPER_DIR"' EXIT

resolve_clang_wrapper() {
  local rust_triple="$1"
  local clang_target="$2"
  local kind="$3" # clang or clang++
  local direct="$LLVM_BIN/${rust_triple}-${kind}"
  if [ -x "$direct" ]; then
    printf '%s\n' "$direct"
    return 0
  fi
  local wrapper="$WRAPPER_DIR/${rust_triple}-${kind}"
  cat > "$wrapper" <<EOF
#!/usr/bin/env sh
exec "$LLVM_BIN/$kind" -target "$clang_target" --sysroot="$SYSROOT" -D__MUSL__ "\$@"
EOF
  chmod +x "$wrapper"
  printf '%s\n' "$wrapper"
}

AARCH64_CC="$(resolve_clang_wrapper aarch64-unknown-linux-ohos aarch64-linux-ohos clang)"
AARCH64_CXX="$(resolve_clang_wrapper aarch64-unknown-linux-ohos aarch64-linux-ohos clang++)"
X86_64_CC="$(resolve_clang_wrapper x86_64-unknown-linux-ohos x86_64-linux-ohos clang)"
X86_64_CXX="$(resolve_clang_wrapper x86_64-unknown-linux-ohos x86_64-linux-ohos clang++)"

export CC_aarch64_unknown_linux_ohos="$AARCH64_CC"
export CXX_aarch64_unknown_linux_ohos="$AARCH64_CXX"
export AR_aarch64_unknown_linux_ohos="$LLVM_BIN/llvm-ar"
export RANLIB_aarch64_unknown_linux_ohos="$LLVM_BIN/llvm-ranlib"
export CFLAGS_aarch64_unknown_linux_ohos="--sysroot=$SYSROOT"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$AARCH64_CC"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="-C link-arg=--sysroot=$SYSROOT -C link-arg=-Wl,-z,max-page-size=16384"

export CC_x86_64_unknown_linux_ohos="$X86_64_CC"
export CXX_x86_64_unknown_linux_ohos="$X86_64_CXX"
export AR_x86_64_unknown_linux_ohos="$LLVM_BIN/llvm-ar"
export RANLIB_x86_64_unknown_linux_ohos="$LLVM_BIN/llvm-ranlib"
export CFLAGS_x86_64_unknown_linux_ohos="--sysroot=$SYSROOT"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_LINKER="$X86_64_CC"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="-C link-arg=--sysroot=$SYSROOT -C link-arg=-Wl,-z,max-page-size=16384"

cd "$REPO_ROOT"
for cpu in "${targets[@]}"; do
  triple="$cpu-unknown-linux-ohos"
  abi="$cpu"
  [[ $cpu != aarch64 ]] || abi=arm64-v8a
  rustup target add "$triple"
  cargo build --lib --release --features "$features" -p orbit-ffi-ohos --target "$triple"
  mkdir -p "apps/harmony/entry/libs/$abi"
  cp "${CARGO_TARGET_DIR:-target}/$triple/release/liborbit_ohos.so" "apps/harmony/entry/libs/$abi/"
  echo "Built apps/harmony/entry/libs/$abi/liborbit_ohos.so"
done
