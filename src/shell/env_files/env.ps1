# zv shell setup for PowerShell
# To permanently set environment variables in PowerShell, run as Administrator:
# [Environment]::SetEnvironmentVariable("ZV_DIR", "{zv_dir}", "User")
# [Environment]::SetEnvironmentVariable("PATH", "{zv_bin_path}{zv_path_separator}$env:PATH", "User")

$env:ZV_DIR = "{zv_dir}"
if ($env:PATH -notlike "*{zv_bin_path}*") {{
    $env:PATH = "{zv_bin_path}{zv_path_separator}$env:PATH"
}}
