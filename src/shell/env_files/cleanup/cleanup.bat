REM zv cleanup script for Command Prompt
set "ZV_DIR="
set "PATH=%PATH:{zv_bin_path}{zv_path_separator}=%"
set "PATH=%PATH:{zv_path_separator}{zv_bin_path}=%"

echo zv environment cleaned up for current session
echo To permanently remove, run as Administrator:
echo setx ZV_DIR "" /M
