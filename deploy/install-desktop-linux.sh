#!/bin/sh
# One-line Orbit Tauri v2 Linux Desktop App installer (x86_64 & ARM64).
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install-desktop-linux.sh | sh

set -eu

REPO="${ORBIT_REPO:-danielgaoyy/orbit}"
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "${OS}" != "Linux" ]; then
  echo "This desktop installer is for Linux (detected ${OS})." >&2
  exit 1
fi

case "${ARCH}" in
  x86_64|amd64)
    ASSET="Orbit-Linux-x64.deb"
    ;;
  aarch64|arm64)
    ASSET="Orbit-Linux-arm64.deb"
    ;;
  *)
    echo "Unsupported Linux architecture: ${ARCH}" >&2
    exit 1
    ;;
esac

TMP_DEB="$(mktemp "/tmp/orbit-desktop.XXXXXX.deb")"
trap 'rm -f "${TMP_DEB}"' EXIT

URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
echo "Downloading Orbit Tauri v2 Linux Desktop (${ARCH}) from ${URL} ..."
curl -fL --retry 3 -o "${TMP_DEB}" "${URL}"

if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update -y
  sudo apt-get install -y "${TMP_DEB}"
  echo "[✓] Installed Orbit Desktop (Tauri v2) via apt."
else
  echo "Downloaded ${TMP_DEB}. Install it with your distro package manager or extract data.tar.*"
fi
