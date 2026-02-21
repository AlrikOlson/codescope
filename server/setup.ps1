# CodeScope — Windows PowerShell Bootstrap
# Finds bash (Git Bash, WSL, or MSYS2) and delegates to setup.sh.
#
# Usage:
#   irm https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.ps1 | iex

$ErrorActionPreference = "Stop"
$Repo = "AlrikOlson/codescope"
$Branch = "master"
$SetupUrl = "https://raw.githubusercontent.com/$Repo/$Branch/server/setup.sh"

# Find bash — Git for Windows, WSL, MSYS2, Cygwin
function Find-Bash {
    # 1. Git for Windows (most common)
    $gitBash = "${env:ProgramFiles}\Git\bin\bash.exe"
    if (Test-Path $gitBash) { return $gitBash }
    $gitBash = "${env:ProgramFiles(x86)}\Git\bin\bash.exe"
    if (Test-Path $gitBash) { return $gitBash }

    # 2. In PATH (MSYS2, Cygwin, or manually added)
    $inPath = Get-Command bash -ErrorAction SilentlyContinue
    if ($inPath) { return $inPath.Source }

    # 3. WSL
    $wsl = Get-Command wsl -ErrorAction SilentlyContinue
    if ($wsl) { return "wsl" }

    return $null
}

$bash = Find-Bash

if (-not $bash) {
    Write-Host "  ! bash not found. Install Git for Windows first:" -ForegroundColor Yellow
    Write-Host "    https://git-scm.com/downloads/win" -ForegroundColor White
    Write-Host ""
    Write-Host "  Then re-run:" -ForegroundColor White
    Write-Host "    irm https://raw.githubusercontent.com/$Repo/$Branch/server/setup.ps1 | iex" -ForegroundColor Cyan
    exit 1
}

# Download and run setup.sh, forwarding all arguments
$tmpScript = Join-Path $env:TEMP "codescope-setup-$(Get-Random).sh"
try {
    Invoke-WebRequest -Uri $SetupUrl -OutFile $tmpScript -TimeoutSec 30
} catch {
    Write-Host "  ! Failed to download setup script. Check your internet connection." -ForegroundColor Red
    exit 1
}

$passArgs = $args -join " "

if ($bash -eq "wsl") {
    $wslPath = wsl wslpath -u ($tmpScript -replace '\\','/')
    wsl bash $wslPath $passArgs
} else {
    & $bash $tmpScript $passArgs
}

Remove-Item -Force $tmpScript -ErrorAction SilentlyContinue
