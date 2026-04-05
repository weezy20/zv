#!/usr/bin/env bash
# npm postinstall — downloads the correct zv binary from GitHub Releases.
#
# Environment:
#   ZV_VERSION  — version to install (set during npm pack, defaults to latest)
set -euo pipefail

VERSION="${ZV_VERSION:-}"
REPO="weezy20/zv"
GITHUB_API="https://api.github.com/repos/${REPO}"
GITHUB_RELEASE="https://github.com/${REPO}/releases"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
    Linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
    Linux-arm64)    TARGET="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64)  TARGET="x86_64-apple-darwin" ;;
    Darwin-arm64)   TARGET="aarch64-apple-darwin" ;;
    *) echo "zv: unsupported platform ${OS}-${ARCH}, skipping install"; exit 0 ;;
esac

if [[ -z "${VERSION}" ]]; then
    VERSION="$(curl -fsSL "${GITHUB_API}/releases/latest" | grep '"tag_name"' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
fi

ARCHIVE_URL="${GITHUB_RELEASE}/download/${VERSION}/zv-${TARGET}.tar.gz"

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/zv"
BIN_DIR="${DATA_DIR}/bin"
PUB_BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

echo "zv: downloading ${VERSION} for ${TARGET}..."
curl -fsSL "${ARCHIVE_URL}" | tar xzf - -C "${TMPDIR}"

mkdir -p "${BIN_DIR}"
mv "${TMPDIR}/zv-${TARGET}/zv" "${BIN_DIR}/zv"
chmod +x "${BIN_DIR}/zv"

mkdir -p "${PUB_BIN_DIR}"
ln -sf "${BIN_DIR}/zv" "${PUB_BIN_DIR}/zv"

echo "zv: installed ${VERSION} to ${BIN_DIR}/zv (symlink: ${PUB_BIN_DIR}/zv)"
