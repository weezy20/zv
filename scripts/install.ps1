# zv installer for Windows
#
# Usage:
#   irm https://raw.githubusercontent.com/weezy20/zv/main/scripts/install.ps1 | iex
#   irm https://raw.githubusercontent.com/weezy20/zv/main/scripts/install.ps1 | iex -Args '-Version','v0.9.2'
#
# Options:
#   -Version <tag>     Install a specific version (default: latest)
#   -SkipChecksum      Skip SHA-256 checksum verification
#   -Help              Show this help message
param(
    [string]$Version = '',
    [switch]$SkipChecksum,
    [switch]$Help
)

if ($Help) {
    Write-Host "Usage: irm <url> | iex [-Version <tag>] [-SkipChecksum]"
    exit 0
}

$ErrorActionPreference = 'Stop'

$Repo = 'weezy20/zv'
$GithubApi = "https://api.github.com/repos/$Repo"
$GithubRelease = "https://github.com/$Repo/releases"

# ── Detect platform ─────────────────────────────────────────────────────────

$Target = 'x86_64-pc-windows-msvc'

# ── Resolve version ─────────────────────────────────────────────────────────

if (-not $Version) {
    Write-Host "=> Fetching latest version..." -ForegroundColor Cyan
    $latest = Invoke-RestMethod -Uri "$GithubApi/releases/latest" -Headers @{ 'User-Agent' = 'zv-installer' }
    $Version = $latest.tag_name
    if (-not $Version) {
        Write-Host "Could not determine latest version." -ForegroundColor Red
        exit 1
    }
}

Write-Host "=> Installing zv $Version for $Target..." -ForegroundColor Cyan

# ── Download ─────────────────────────────────────────────────────────────────

$TmpDir = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "zv-install-$(Get-Random)")
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

$ArchiveName = "zv-$Target.zip"
$ChecksumName = "zv-$Target.zip.sha256"
$ArchiveUrl = "$GithubRelease/download/$Version/$ArchiveName"
$ChecksumUrl = "$GithubRelease/download/$Version/$ChecksumName"
$ArchivePath = [System.IO.Path]::Combine($TmpDir, $ArchiveName)

Write-Host "=> Downloading $ArchiveName..."
Invoke-WebRequest -Uri $ArchiveUrl -OutFile $ArchivePath -UseBasicParsing

# ── Verify checksum ──────────────────────────────────────────────────────────

if (-not $SkipChecksum) {
    Write-Host "=> Verifying checksum..."
    try {
        $checksumFile = [System.IO.Path]::Combine($TmpDir, $ChecksumName)
        Invoke-WebRequest -Uri $ChecksumUrl -OutFile $checksumFile -UseBasicParsing
        $expected = (Get-Content $checksumFile -Raw).Split(' ')[0].Trim()
        $hash = (Get-FileHash -Path $ArchivePath -Algorithm SHA256).Hash
        if ($hash -ne $expected) {
            Write-Host "Checksum mismatch! Expected $expected, got $hash" -ForegroundColor Red
            exit 1
        }
        Write-Host "   Checksum verified" -ForegroundColor Green
    } catch {
        Write-Host "   Checksum file not found — skipping verification" -ForegroundColor Yellow
    }
}

# ── Extract ──────────────────────────────────────────────────────────────────

Write-Host "=> Extracting..."
Expand-Archive -Path $ArchivePath -DestinationPath $TmpDir -Force
$Binary = [System.IO.Path]::Combine($TmpDir, "zv-$Target", 'zv.exe')
if (-not (Test-Path $Binary)) {
    Write-Host "Binary not found in archive (expected zv-$Target\zv.exe)" -ForegroundColor Red
    exit 1
}

# ── Determine install path ───────────────────────────────────────────────────

$ZvDir = if ($env:ZV_DIR) { $env:ZV_DIR } else { [System.IO.Path]::Combine($env:USERPROFILE, '.zv') }
$BinDir = [System.IO.Path]::Combine($ZvDir, 'bin')
$DestExe = [System.IO.Path]::Combine($BinDir, 'zv.exe')

# ── Install binary ───────────────────────────────────────────────────────────

Write-Host "=> Installing to $BinDir..."
New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
Copy-Item -Path $Binary -Destination $DestExe -Force
Write-Host "   Binary installed to $DestExe" -ForegroundColor Green

# ── Add to PATH (registry) ───────────────────────────────────────────────────

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$normalizedBinDir = $BinDir.TrimEnd('\')
$pathEntries = $userPath -split ';' | Where-Object { $_.Trim() }
$alreadyInPath = $pathEntries | Where-Object {
    $_.Trim().TrimEnd('\') -ieq $normalizedBinDir
}

if (-not $alreadyInPath) {
    $newPath = if ($userPath) { "$normalizedBinDir;$userPath" } else { $normalizedBinDir }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    $env:Path = "$normalizedBinDir;$($env:Path)"
    Write-Host "   Added $normalizedBinDir to user PATH" -ForegroundColor Green
} else {
    Write-Host "   $BinDir already in PATH" -ForegroundColor Green
}

# ── Cleanup ──────────────────────────────────────────────────────────────────

Remove-Item -Path $TmpDir -Recurse -Force

# ── Done ─────────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "   zv $Version installed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "  Binary:  $DestExe"
Write-Host "  Data:    $ZvDir"
Write-Host ""
Write-Host "  Next steps:"
Write-Host "    zv setup    -- configure your shell environment"
Write-Host "    zv sync     -- fetch zig indices and mirrors"
Write-Host "    zv install  -- install a zig version"
Write-Host ""
