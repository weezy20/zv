#!/usr/bin/env bash
# prepack.sh — inject version into package.json before npm publish.
#
# Usage: ZV_VERSION=v0.9.2 bash prepack.sh
set -euo pipefail

VERSION="${ZV_VERSION:-}"
VERSION="${VERSION#v}"

if [[ -z "${VERSION}" ]]; then
    echo "ZV_VERSION not set" >&2
    exit 1
fi

DIR="$(cd "$(dirname "$0")" && pwd)"

# Update version in package.json
cd "${DIR}"
node -e "
const fs = require('fs');
const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
pkg.version = '${VERSION}';
fs.writeFileSync('package.json', JSON.stringify(pkg, null, 2) + '\n');
"

echo "package.json version set to ${VERSION}"
