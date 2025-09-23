# --- 1: Resolve ZV_DIR before removing it ---
$zvDir = if ($env:ZV_DIR) {
    $env:ZV_DIR
} else {
    [Environment]::GetEnvironmentVariable("ZV_DIR", "User")
}

# Determine bin path to remove
if ($zvDir) {
    $binPath = Join-Path $zvDir "bin"
} else {
    $binPath = Join-Path "$env:USERPROFILE\.zv" "bin"
}

# Normalize path for comparison
$normalizedBinPath = (Resolve-Path -LiteralPath $binPath -ErrorAction SilentlyContinue)?.ProviderPath
if (-not $normalizedBinPath) {
    # Fall back to as-is if directory does not exist
    $normalizedBinPath = [System.IO.Path]::GetFullPath($binPath)
}

function Remove-PathEntry {
    param(
        [string]$PathString,
        [string]$EntryToRemove
    )

    $entries = $PathString -split ';' | ForEach-Object { $_.Trim() }
    $filtered = $entries | Where-Object {
        $_ -and (-not ([System.IO.Path]::GetFullPath($_).TrimEnd('\') -ieq $EntryToRemove.TrimEnd('\')))
    }
    return ($filtered -join ';')
}

# --- 2: Remove binPath from PATH (persistently for user) ---
$oldPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($oldPath) {
    $newPath = Remove-PathEntry -PathString $oldPath -EntryToRemove $normalizedBinPath
    if ($newPath -ne $oldPath) {
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "✅ Removed $normalizedBinPath from user PATH."
    } else {
        Write-Host "ℹ️ $normalizedBinPath not found in user PATH."
    }
}

# --- 3: Remove binPath from current session PATH ---
$sessionOldPath = $env:Path
$env:Path = Remove-PathEntry -PathString $sessionOldPath -EntryToRemove $normalizedBinPath
if ($env:Path -ne $sessionOldPath) {
    Write-Host "✅ Removed $normalizedBinPath from current session PATH."
}

# --- 4: Now remove ZV_DIR env vars ---
if ($env:ZV_DIR) {
    Remove-Item Env:ZV_DIR -ErrorAction SilentlyContinue
    Write-Host "✅ Removed ZV_DIR from current session."
}

if ($zvDir) {
    [Environment]::SetEnvironmentVariable("ZV_DIR", $null, "User")
    Write-Host "✅ Removed ZV_DIR from user environment."
}
