#!/usr/bin/env bash
# run-zv.sh — thin wrapper that execs the real zv binary.
# This file is installed as the npm bin entry.
set -euo pipefail
exec "${XDG_DATA_HOME:-$HOME/.local/share}/zv/bin/zv" "$@"
