# Remove ZV_DIR environment variable from current session
if ($env:ZV_DIR) {
    Remove-Item Env:ZV_DIR
    Write-Host "Removed ZV_DIR from current session."
}

# Remove ZV_DIR from user environment variables (persistently)
$zvDir = [Environment]::GetEnvironmentVariable("ZV_DIR", "User")
if ($zvDir) {
    [Environment]::SetEnvironmentVariable("ZV_DIR", $null, "User")
    Write-Host "Removed ZV_DIR from user environment."
}

# Determine bin path to remove from PATH
if ($zvDir) {
    $binPath = Join-Path $zvDir "bin"
} else {
    $binPath = "$env:USERPROFILE\.zv\bin"
}

# Remove binPath from PATH (persistently for user)
$oldPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($oldPath) {
    $newPath = ($oldPath -split ';') | Where-Object { $_ -ne $binPath } | -join ';'
    if ($newPath -ne $oldPath) {
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "Removed $binPath from user PATH."
    } else {
        Write-Host "$binPath not found in user PATH."
    }
}

# Remove binPath from current session PATH
$env:Path = ($env:Path -split ';') | Where-Object { $_ -ne $binPath } | -join ';'
Write-Host "Removed $binPath from current session PATH (if present)."