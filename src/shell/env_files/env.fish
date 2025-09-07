#!/usr/bin/env fish
# zv shell setup for Fish shell
set -gx ZV_DIR "{zv_dir}"
if not contains "{zv_bin_path}" $PATH
    set -gx PATH "{zv_bin_path}" $PATH
end
