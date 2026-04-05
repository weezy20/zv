#!/usr/bin/env bash
# zv installer — downloads and installs zv to XDG-compliant directories.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/weezy20/zv/main/scripts/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/weezy20/zv/main/scripts/install.sh | bash -s -- --version v0.9.2
#
# Options:
#   --version <tag>   Install a specific version (default: latest)
#   --skip-checksum   Skip SHA-256 checksum verification
#   -h, --help        Show this help message
set -euo pipefail

REPO="weezy20/zv"
GITHUB_API="https://api.github.com/repos/${REPO}"
GITHUB_RELEASE="https://github.com/${REPO}/releases"

VERSION=""
SKIP_CHECKSUM=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)  VERSION="$2"; shift 2 ;;
        --skip-checksum) SKIP_CHECKSUM=true; shift ;;
        -h|--help)
            echo "Usage: curl -fsSL <url> | bash -s -- [--version <tag>] [--skip-checksum]"
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

say()   { printf '\033[1;36m=>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m!\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

# ── Detect platform ──────────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
    Linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
    Linux-arm64)    TARGET="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64)  TARGET="x86_64-apple-darwin" ;;
    Darwin-arm64)   TARGET="aarch64-apple-darwin" ;;
    *) die "Unsupported platform: ${OS}-${ARCH}. Please open an issue at https://github.com/${REPO}/issues" ;;
esac

# ── Resolve version ──────────────────────────────────────────────────────────

if [[ -z "${VERSION}" ]]; then
    say "Fetching latest version..."
    VERSION="$(curl -fsSL "${GITHUB_API}/releases/latest" | grep '"tag_name"' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
    if [[ -z "${VERSION}" ]]; then
        die "Could not determine latest version."
    fi
fi

say "Installing zv ${VERSION} for ${TARGET}..."

# ── Download ─────────────────────────────────────────────────────────────────

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

ARCHIVE_NAME="zv-${TARGET}.tar.gz"
CHECKSUM_NAME="zv-${TARGET}.tar.gz.sha256"
ARCHIVE_URL="${GITHUB_RELEASE}/download/${VERSION}/${ARCHIVE_NAME}"
CHECKSUM_URL="${GITHUB_RELEASE}/download/${VERSION}/${CHECKSUM_NAME}"

say "Downloading ${ARCHIVE_NAME}..."
curl -fsSL "${ARCHIVE_URL}" -o "${TMPDIR}/${ARCHIVE_NAME}" || die "Download failed: ${ARCHIVE_URL}"

# ── Verify checksum ──────────────────────────────────────────────────────────

if [[ "${SKIP_CHECKSUM}" == "false" ]]; then
    say "Verifying checksum..."
    if curl -fsSL "${CHECKSUM_URL}" -o "${TMPDIR}/${CHECKSUM_NAME}" 2>/dev/null; then
        EXPECTED="$(cut -d' ' -f1 < "${TMPDIR}/${CHECKSUM_NAME}")"
        ACTUAL="$(sha256sum "${TMPDIR}/${ARCHIVE_NAME}" | cut -d' ' -f1)"
        if [[ "${EXPECTED}" != "${ACTUAL}" ]]; then
            die "Checksum mismatch! Expected ${EXPECTED}, got ${ACTUAL}"
        fi
        ok "Checksum verified"
    else
        warn "Checksum file not found — skipping verification"
    fi
fi

# ── Extract ──────────────────────────────────────────────────────────────────

say "Extracting..."
tar xzf "${TMPDIR}/${ARCHIVE_NAME}" -C "${TMPDIR}"
BINARY="${TMPDIR}/zv-${TARGET}/zv"
if [[ ! -f "${BINARY}" ]]; then
    die "Binary not found in archive (expected zv-${TARGET}/zv)"
fi

# ── Determine XDG paths ─────────────────────────────────────────────────────

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/zv"
BIN_DIR="${DATA_DIR}/bin"
PUB_BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

# ── Install binary ───────────────────────────────────────────────────────────

say "Installing to ${BIN_DIR}..."
mkdir -p "${BIN_DIR}"
cp -f "${BINARY}" "${BIN_DIR}/zv"
chmod +x "${BIN_DIR}/zv"
ok "Binary installed to ${BIN_DIR}/zv"

# ── Create public symlink ────────────────────────────────────────────────────

mkdir -p "${PUB_BIN_DIR}"

create_or_update_symlink() {
    local target="$1" link="$2"
    if [[ -L "${link}" ]]; then
        rm -f "${link}"
    elif [[ -e "${link}" ]]; then
        warn "${link} exists and is not a symlink — skipping symlink creation"
        return
    fi
    ln -s "${target}" "${link}"
}

create_or_update_symlink "${BIN_DIR}/zv" "${PUB_BIN_DIR}/zv"
ok "Symlink created: ${PUB_BIN_DIR}/zv → ${BIN_DIR}/zv"

# ── PATH check ───────────────────────────────────────────────────────────────

if [[ ":${PATH}:" != *":${PUB_BIN_DIR}:"* ]]; then
    echo ""
    warn "${PUB_BIN_DIR} is not in your PATH."
    echo ""
    echo "  Add this line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo ""
    echo "    export PATH=\"${PUB_BIN_DIR}:\$PATH\""
    echo ""
    echo "  Or run:  zv setup"
    echo ""
fi

# ── Done ─────────────────────────────────────────────────────────────────────

echo ""
ok "zv ${VERSION} installed successfully!"
echo ""
echo "  Binary:   ${BIN_DIR}/zv"
echo "  Symlink:  ${PUB_BIN_DIR}/zv"
echo "  Data:     ${DATA_DIR}"
echo ""
echo "  Next steps:"
echo "    zv setup    — configure your shell environment"
echo "    zv sync     — fetch zig indices and mirrors"
echo "    zv install  — install a zig version"
echo ""
