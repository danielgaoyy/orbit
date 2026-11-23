#!/bin/sh
# Universal 1-line Orbit CLI / Daemon / Relay installer for Linux & macOS.
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh
# Optional overrides:
#   ORBIT_GITHUB_REPO=owner/repo  (default: danielgaoyy/orbit)
#   ORBIT_VERSION=0.9.5           (default: latest GitHub release)

set -eu

REPO="${ORBIT_GITHUB_REPO:-${ORBIT_REPO:-danielgaoyy/orbit}}"
NL='
'
CR="$(printf '\r')"
case "${REPO}" in
  *"${NL}"*|*"${CR}"*|*..*|./*|*/.|*/*/*)
    echo "Error: invalid Orbit GitHub repository '${REPO}' (expected owner/repo)" >&2
    exit 1
    ;;
esac
if ! printf '%s\n' "${REPO}" | LC_ALL=C grep -Eq '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$'; then
  echo "Error: invalid Orbit GitHub repository '${REPO}' (expected owner/repo)" >&2
  exit 1
fi

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

INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "${INSTALL_DIR}"
TMP_BIN="$(mktemp "${INSTALL_DIR}/orbit.tmp.XXXXXX")"
TMP_SUMS="$(mktemp "${INSTALL_DIR}/orbit.sha256.XXXXXX")"
trap 'rm -f "${TMP_BIN}" "${TMP_SUMS}"' EXIT

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

echo "Downloading Orbit ${TAG} (${OS}/${ARCH}) from ${URL} ..."
curl -fsSLg --proto '=https' --proto-redir '=https' --retry 3 -o "${TMP_BIN}" "${URL}"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(sha256sum "${TMP_BIN}" | awk '{print tolower($1)}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(shasum -a 256 "${TMP_BIN}" | awk '{print tolower($1)}')"
else
  echo "Error: neither sha256sum nor shasum is installed; refusing to install unverified binary." >&2
  exit 1
fi

if [ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]; then
  echo "Error: SHA-256 checksum mismatch for ${ASSET}!" >&2
  echo "  expected: ${EXPECTED_SHA256}" >&2
  echo "  actual:   ${ACTUAL_SHA256}" >&2
  exit 1
fi
echo "[✓] Verified SHA-256 checksum for ${ASSET} (${ACTUAL_SHA256})"

chmod 755 "${TMP_BIN}"
mv -f "${TMP_BIN}" "${INSTALL_DIR}/orbit"
INSTALLED_VER="$("${INSTALL_DIR}/orbit" --version)"

# Create /usr/local/bin/orbit symlink only when absent (never overwrite an existing file or symlink).
if [ -d /usr/local/bin ] && [ ! -e /usr/local/bin/orbit ] && [ ! -L /usr/local/bin/orbit ]; then
  if [ -w /usr/local/bin ]; then
    ln -s "${INSTALL_DIR}/orbit" /usr/local/bin/orbit
  else
    case ":${PATH}:" in
      *":${INSTALL_DIR}:"*) ;;
      *)
        if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
          sudo ln -s "${INSTALL_DIR}/orbit" /usr/local/bin/orbit
        fi
        ;;
    esac
  fi
fi

resolve_canonical_path() {
  target="$1"
  depth=0
  while [ -L "${target}" ] && [ "${depth}" -lt 16 ]; do
    link="$(readlink "${target}" 2>/dev/null)" || return 1
    case "${link}" in
      /*) target="${link}" ;;
      *)  target="$(dirname "${target}")/${link}" ;;
    esac
    depth=$((depth + 1))
  done
  [ ! -L "${target}" ] && [ -e "${target}" ] || return 1
  dir="$(cd -P "$(dirname "${target}")" 2>/dev/null && pwd -P)" || return 1
  printf '%s/%s\n' "${dir}" "$(basename "${target}")"
}

GLOBAL_SYMLINK_ACTIVE=0
if [ -L /usr/local/bin/orbit ]; then
  RESOLVED_GLOBAL="$(resolve_canonical_path /usr/local/bin/orbit 2>/dev/null || true)"
  RESOLVED_LOCAL="$(resolve_canonical_path "${INSTALL_DIR}/orbit" 2>/dev/null || true)"
  if [ -n "${RESOLVED_GLOBAL}" ] && [ "${RESOLVED_GLOBAL}" = "${RESOLVED_LOCAL}" ]; then
    GLOBAL_SYMLINK_ACTIVE=1
  fi
fi

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*)
    echo "[✓] Installed orbit -> ${INSTALL_DIR}/orbit (${INSTALLED_VER})"
    ;;
  *)
    if [ "${GLOBAL_SYMLINK_ACTIVE}" = "1" ]; then
      echo "[✓] Installed orbit -> ${INSTALL_DIR}/orbit (symlinked at /usr/local/bin/orbit; ${INSTALLED_VER})"
    else
      echo "[✓] Installed orbit -> ${INSTALL_DIR}/orbit (${INSTALLED_VER})"
      echo "    Add to PATH: export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    ;;
esac
