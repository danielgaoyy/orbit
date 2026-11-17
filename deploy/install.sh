#!/bin/sh
# Universal 1-line Orbit CLI / Daemon / Relay installer for Linux & macOS.
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh

set -eu

REPO="${ORBIT_REPO:-danielgaoyy/orbit}"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
  Linux-x86_64|Linux-amd64)
    ASSET="orbit-cli-linux-x64"
    ;;
  Linux-aarch64|Linux-arm64)
    ASSET="orbit-cli-linux-arm64"
    ;;
  Darwin-arm64|Darwin-aarch64)
    ASSET="orbit-cli-macos-arm64"
    ;;
  *)
    echo "Unsupported platform: ${OS}/${ARCH}. Please build from source with 'cargo build --release -p orbit-cli'." >&2
    exit 1
    ;;
esac

INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "${INSTALL_DIR}"
TMP_BIN="$(mktemp "${INSTALL_DIR}/orbit.tmp.XXXXXX")"
trap 'rm -f "${TMP_BIN}"' EXIT

URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
echo "Downloading Orbit (${OS}/${ARCH}) from ${URL} ..."
curl -fL --retry 3 -o "${TMP_BIN}" "${URL}"
chmod 755 "${TMP_BIN}"
mv -f "${TMP_BIN}" "${INSTALL_DIR}/orbit"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*)
    echo "[✓] Installed orbit -> ${INSTALL_DIR}/orbit ($( "${INSTALL_DIR}/orbit" --version ))"
    ;;
  *)
    if [ -w /usr/local/bin ]; then
      ln -sf "${INSTALL_DIR}/orbit" /usr/local/bin/orbit
      echo "[✓] Installed orbit -> /usr/local/bin/orbit ($( /usr/local/bin/orbit --version ))"
    elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
      sudo ln -sf "${INSTALL_DIR}/orbit" /usr/local/bin/orbit
      echo "[✓] Installed orbit -> /usr/local/bin/orbit ($( /usr/local/bin/orbit --version ))"
    else
      echo "[✓] Installed orbit -> ${INSTALL_DIR}/orbit"
      echo "    Add to PATH: export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    ;;
esac
