#!/usr/bin/env bash
# Generate a Homebrew formula for zv.
#
# Usage: ./generate-formula.sh <version>
#
# The version must be a git tag (e.g. v0.9.2).
# The formula is written to stdout.
set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <version>" >&2
    exit 1
fi

VERSION="${1#v}"
REPO="weezy20/zv"
BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"

checksum_for() {
    local target="$1"
    local url="${BASE_URL}/zv-${target}.tar.gz.sha256"
    curl -fsSL "${url}" | cut -d' ' -f1
}

INTEL_SHA="$(checksum_for x86_64-apple-darwin)"
ARM_SHA="$(checksum_for aarch64-apple-darwin)"

cat <<FORMULA
class Zv < Formula
  desc "Ziglang Version Manager and Project Starter"
  homepage "https://github.com/${REPO}"
  url "${BASE_URL}/zv-x86_64-apple-darwin.tar.gz"
  sha256 "${INTEL_SHA}"
  version "${VERSION}"
  license "MIT"

  on_arm do
    url "${BASE_URL}/zv-aarch64-apple-darwin.tar.gz"
    sha256 "${ARM_SHA}"
  end

  def install
    mkdir_p libexec
    mv "zv", "\#{libexec}/zv"
    chmod 0755, "\#{libexec}/zv"
    mkdir_p bin
    ln_s "\#{libexec}/zv", "\#{bin}/zv"
  end

  def caveats
    <<~EOS
      zv is installed at \#{libexec}/zv with a symlink at \#{bin}/zv.

      Data directory: ~/.local/share/zv
      Run `zv setup` to configure your shell environment.
    EOS
  end

  test do
    assert_match "zv", shell_output("\#{bin}/zv --version")
  end
end
FORMULA
