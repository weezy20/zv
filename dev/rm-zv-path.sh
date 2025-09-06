#!/bin/zsh
# Remove default zv_bin_path for testing
export PATH="$(echo "$PATH" | tr ':' '\n' | grep -v "$HOME/.zv/bin" | paste -sd: -)"
