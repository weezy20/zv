#!/usr/bin/env fish
# zv shell setup for Fish shell
{zv_dir_export}
if not contains "{zv_bin_path}" $PATH
    set -gx PATH "{zv_bin_path}" $PATH
end
