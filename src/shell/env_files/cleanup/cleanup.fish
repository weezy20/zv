#!/usr/bin/env fish
# zv cleanup script for Fish shell

set -e ZV_DIR
if set -l index (contains -i "{zv_bin_path}" $PATH)
    set -e PATH[$index]
end

echo "zv environment cleaned up"
