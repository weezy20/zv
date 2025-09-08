#!/bin/csh  
# zv cleanup script for tcsh/csh

unsetenv ZV_DIR
setenv PATH `echo $PATH | sed 's|{zv_bin_path}:||g' | sed 's|:{zv_bin_path}||g'`

echo "zv environment cleaned up"
