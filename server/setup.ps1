# CodeScope — Windows PowerShell Install Script
# Downloads a pre-built binary. No compilation needed.
#
# Usage:
#   irm https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.ps1 | iex

$ErrorActionPreference = "Stop"
$Repo = "AlrikOlson/codescope"

# --- Install directory ---
$InstallDir = Join-Path $env:LOCALAPPDATA "codescope\bin"

function Write-Info($msg)  { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)    { Write-Host " OK $msg" -ForegroundColor Green }
function Write-Err($msg)   { Write-Host "ERR $msg" -ForegroundColor Red }

# --- Detect architecture ---
function Get-Platform {
    $arch = if ([Environment]::Is64BitOperatingSystem) {
        if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "aarch64" } else { "x86_64" }
    } else {
        throw "32-bit Windows is not supported"
    }
    return "windows-$arch"
}

# --- Download and install ---
function Install-CodeScope {
    $platform = Get-Platform
    Write-Info "Installing CodeScope..."
    Write-Host ""

    # Fetch latest release
    Write-Info "Checking for latest stable release..."
    $releaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
    try {
        $release = Invoke-RestMethod -Uri $releaseUrl -TimeoutSec 15
    } catch {
        Write-Err "Could not reach GitHub. Check your internet connection."
        return
    }

    $tag = $release.tag_name
    if (-not $tag) {
        Write-Err "Could not find a release to download"
        return
    }

    $archive = "codescope-server-${platform}.zip"
    $url = "https://github.com/$Repo/releases/download/$tag/$archive"

    Write-Info "Downloading CodeScope $tag ($platform)..."

    $tmpDir = Join-Path $env:TEMP "codescope-install-$(Get-Random)"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
    $archivePath = Join-Path $tmpDir $archive

    try {
        Invoke-WebRequest -Uri $url -OutFile $archivePath -TimeoutSec 120
    } catch {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
        Write-Err "Download failed. Your platform ($platform) may not have a pre-built binary."
        return
    }

    # Extract
    $extractDir = Join-Path $tmpDir "extracted"
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    # Find binary
    $binary = Get-ChildItem -Path $extractDir -Filter "codescope-server.exe" -Recurse | Select-Object -First 1
    if (-not $binary) {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
        Write-Err "Archive did not contain codescope-server.exe"
        return
    }

    # Install
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $dest = Join-Path $InstallDir "codescope-server.exe"
    # Remove old binary first to avoid locking issues
    Remove-Item -Force $dest -ErrorAction SilentlyContinue
    Copy-Item -Path $binary.FullName -Destination $dest -Force
    Write-Ok "Installed codescope-server.exe -> $InstallDir\"

    # Install helper scripts if present
    foreach ($script in @("codescope-init.exe", "codescope-web.exe")) {
        $found = Get-ChildItem -Path $extractDir -Filter $script -Recurse | Select-Object -First 1
        if ($found) {
            Copy-Item -Path $found.FullName -Destination (Join-Path $InstallDir $script) -Force
        }
    }

    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}

# --- PATH setup ---
function Ensure-Path {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Info "Added $InstallDir to user PATH"
    }
}

# --- Main ---
Install-CodeScope
Ensure-Path

Write-Host ""
Write-Ok "CodeScope installed!"
Write-Host ""
Write-Host "  Next steps:"
Write-Host ""
Write-Host "    1. Set up a project:"
Write-Host "       cd C:\path\to\your\project"
Write-Host "       codescope-server init"
Write-Host ""
Write-Host "    2. Open Claude Code in that directory — CodeScope is ready to use."
Write-Host ""
Write-Host "  Semantic search enabled by default. Disable with --no-semantic if needed."
Write-Host ""

if (-not (Get-Command codescope-server -ErrorAction SilentlyContinue)) {
    Write-Host "  NOTE: Restart your terminal to pick up the new PATH." -ForegroundColor Yellow
    Write-Host ""
}
