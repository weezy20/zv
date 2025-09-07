# zv cleanup script for PowerShell
# Remove zv from environment variables

Remove-Item Env:ZV_DIR -ErrorAction SilentlyContinue
$env:PATH = ($env:PATH -split '{zv_path_separator}' | Where-Object {{ $_ -ne "{zv_bin_path}" }}) -join '{zv_path_separator}'

Write-Host "zv environment cleaned up for current session"
Write-Host "To permanently remove, run as Administrator:"
Write-Host "[Environment]::SetEnvironmentVariable('ZV_DIR', `$null, 'User')"
Write-Host "Update PATH manually in System Properties -> Environment Variables"
