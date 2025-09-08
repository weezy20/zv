#!/bin/csh
# zv shell setup for tcsh/csh
setenv ZV_DIR "{zv_dir}"
echo "{zv_path_separator}${PATH}{zv_path_separator}" | grep -q "{zv_path_separator}{zv_bin_path}{zv_path_separator}" || setenv PATH "{zv_bin_path}{zv_path_separator}$PATH"
