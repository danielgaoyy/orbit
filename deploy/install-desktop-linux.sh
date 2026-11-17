#!/bin/sh
# One-line Orbit Native Linux Desktop App installer (x86_64 & ARM64).
# Installs GTK3 native Orbit Desktop GUI + GNOME/Freedesktop Application & Desktop launchers.
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
    ASSET="Orbit-Linux-x64.tar.gz"
    ;;
  aarch64|arm64)
    ASSET="Orbit-Linux-arm64.tar.gz"
    ;;
  *)
    echo "Unsupported Linux architecture: ${ARCH}" >&2
    exit 1
    ;;
esac

APP_DIR="${HOME}/.local/opt/orbit"
BIN_DIR="${HOME}/.local/bin"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"
APPS_DIR="${HOME}/.local/share/applications"

mkdir -p "${APP_DIR}" "${BIN_DIR}" "${ICON_DIR}" "${APPS_DIR}"

TMP_TAR="$(mktemp "/tmp/orbit-desktop.XXXXXX.tar.gz")"
TMP_STAGE="$(mktemp -d "/tmp/orbit-stage.XXXXXX")"
trap 'rm -rf "${TMP_TAR}" "${TMP_STAGE}"' EXIT

URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
echo "Downloading Orbit Linux Desktop (${ARCH}) from ${URL} ..."
curl -fL --retry 3 -o "${TMP_TAR}" "${URL}"

tar -xzf "${TMP_TAR}" -C "${TMP_STAGE}"
rm -rf "${APP_DIR:?}"/*
cp -a "${TMP_STAGE}"/. "${APP_DIR}/"
chmod 755 "${APP_DIR}/orbit-desktop"
ln -sf "${APP_DIR}/orbit-desktop" "${BIN_DIR}/orbit-desktop"

cat > "${ICON_DIR}/dev.orbit.app.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0F172A"/>
      <stop offset="100%" stop-color="#1E293B"/>
    </linearGradient>
    <linearGradient id="ring" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8"/>
      <stop offset="100%" stop-color="#818CF8"/>
    </linearGradient>
  </defs>
  <rect width="256" height="256" rx="56" fill="url(#bg)"/>
  <ellipse cx="128" cy="128" rx="88" ry="44" transform="rotate(-28 128 128)" fill="none" stroke="url(#ring)" stroke-width="12"/>
  <circle cx="128" cy="128" r="30" fill="#38BDF8"/>
  <circle cx="196" cy="92" r="13" fill="#F8FAFC"/>
</svg>
SVG

DESKTOP_FILE="${APPS_DIR}/dev.orbit.app.desktop"
cat > "${DESKTOP_FILE}" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Orbit
GenericName=Multi-Host AI Coding Agent Orchestrator
Comment=Control Antigravity, Codex, and Claude Code across your fleet
Exec=${APP_DIR}/orbit-desktop
Icon=${ICON_DIR}/dev.orbit.app.svg
Terminal=false
Categories=Development;IDE;Utility;
StartupWMClass=dev.orbit.app
EOF
chmod 755 "${DESKTOP_FILE}"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${APPS_DIR}" >/dev/null 2>&1 || true
fi

for DESK_CAND in "${HOME}/Desktop" "${HOME}/桌面"; do
  if [ -d "${DESK_CAND}" ]; then
    SHORTCUT="${DESK_CAND}/Orbit.desktop"
    cp -f "${DESKTOP_FILE}" "${SHORTCUT}"
    chmod 755 "${SHORTCUT}"
    if command -v gio >/dev/null 2>&1; then
      gio set "${SHORTCUT}" metadata::trusted true >/dev/null 2>&1 || true
    fi
    echo "[✓] Created Desktop shortcut -> ${SHORTCUT}"
  fi
done

echo "[✓] Installed Orbit Linux Desktop -> ${APP_DIR}/orbit-desktop"
