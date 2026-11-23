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

if [ -n "${ORBIT_VERSION:-}" ]; then
  case "${ORBIT_VERSION}" in
    v*|V*) TAG="v${ORBIT_VERSION#?}" ;;
    *)     TAG="v${ORBIT_VERSION}" ;;
  esac
else
  RESOLVED_URL="$(curl -fsSLg --proto '=https' --proto-redir '=https' -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")"
  RESOLVED_CLEAN="${RESOLVED_URL%%\?*}"
  RESOLVED_CLEAN="${RESOLVED_CLEAN%/}"
  TAG="${RESOLVED_CLEAN##*/}"
fi

NL='
'
CR="$(printf '\r')"
case "${TAG}" in
  *"${NL}"*|*"${CR}"*|*..*|*/*)
    echo "Error: invalid Orbit release tag '${TAG}'" >&2
    exit 1
    ;;
esac
if ! printf '%s\n' "${TAG}" | LC_ALL=C grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
  echo "Error: invalid Orbit release tag '${TAG}'" >&2
  exit 1
fi

RELEASE_BASE="https://github.com/${REPO}/releases/download/${TAG}"
SUMS_URL="${RELEASE_BASE}/SHA256SUMS.txt"
URL="${RELEASE_BASE}/${ASSET}"

TMP_DEB="$(mktemp "/tmp/orbit-desktop.XXXXXX.deb")"
TMP_SUMS="$(mktemp "/tmp/orbit-desktop.sha256.XXXXXX")"
trap 'rm -f "${TMP_DEB}" "${TMP_SUMS}"' EXIT

echo "Fetching SHA-256 checksums for Orbit ${TAG} from ${SUMS_URL} ..."
curl -fsSLg --proto '=https' --proto-redir '=https' --retry 3 -o "${TMP_SUMS}" "${SUMS_URL}"

EXPECTED_SHA256="$(awk -v asset="${ASSET}" '
  {
    sub(/\r$/, "")
  }
  /^[[:space:]]*($|#)/ { next }
  {
    line = $0
    sub(/^[[:space:]]+/, "", line)
    sp = index(line, " ")
    tb = index(line, "\t")
    if (sp == 0 || (tb > 0 && tb < sp)) sep = tb
    else sep = sp
    if (sep == 0) next
    hash = tolower(substr(line, 1, sep - 1))
    file = substr(line, sep + 1)
    sub(/^[[:space:]]+/, "", file)
    sub(/^\*/, "", file)
    sub(/^\.\//, "", file)
    if (file == asset) {
      if (hash !~ /^[0-9a-f]+$/ || length(hash) != 64) {
        print "Error: malformed SHA-256 digest for " asset > "/dev/stderr"
        exit 2
      }
      if (found != "" && found != hash) {
        print "Error: conflicting duplicate SHA-256 entries for " asset > "/dev/stderr"
        exit 3
      }
      found = hash
    }
  }
  END {
    if (found != "") print found
  }
' "${TMP_SUMS}")"

if ! printf '%s' "${EXPECTED_SHA256}" | grep -Eq '^[0-9a-f]{64}$'; then
  echo "Error: valid SHA-256 entry for ${ASSET} not found in ${SUMS_URL}" >&2
  exit 1
fi

echo "Downloading Orbit Tauri v2 Linux Desktop ${TAG} (${ARCH}) from ${URL} ..."
curl -fsSLg --proto '=https' --proto-redir '=https' --retry 3 -o "${TMP_DEB}" "${URL}"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(sha256sum "${TMP_DEB}" | awk '{print tolower($1)}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(shasum -a 256 "${TMP_DEB}" | awk '{print tolower($1)}')"
else
  echo "Error: neither sha256sum nor shasum is installed; refusing to install unverified package." >&2
  exit 1
fi

if [ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]; then
  echo "Error: SHA-256 checksum mismatch for ${ASSET}!" >&2
  echo "  expected: ${EXPECTED_SHA256}" >&2
  echo "  actual:   ${ACTUAL_SHA256}" >&2
  exit 1
fi
echo "[✓] Verified SHA-256 checksum for ${ASSET} (${ACTUAL_SHA256})"

if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update -y
  sudo apt-get install -y "${TMP_DEB}"
  echo "[✓] Installed Orbit Desktop (Tauri v2) via apt."
else
  trap 'rm -f "${TMP_SUMS}"' EXIT
  echo "Downloaded and verified ${TMP_DEB}. Install it with your distro package manager or extract data.tar.*"
fi
