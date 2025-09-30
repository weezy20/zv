REM zv shell setup for Command Prompt
REM To permanently set environment variables in CMD, run as Administrator:
REM setx ZV_DIR "{zv_dir}" /M
REM setx PATH "{zv_bin_path}{zv_path_separator}%PATH%" /M

{zv_dir_export}
echo {zv_path_separator}%PATH%{zv_path_separator} | find /i "{zv_path_separator}{zv_bin_path}{zv_path_separator}" >nul || set "PATH={zv_bin_path}{zv_path_separator}%PATH%"
