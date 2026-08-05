# Ozygram / Ozymem Automatic Release Installer for Windows
# Run via: powershell -ExecutionPolicy Bypass -File .\install.ps1

$ErrorActionPreference = "Stop"

Write-Host "======================================================" -ForegroundColor Cyan
Write-Host "   Ozymem / Ozygram Release Installer (Windows)      " -ForegroundColor Cyan
Write-Host "======================================================" -ForegroundColor Cyan

$OzyDir = Join-Path $env:USERPROFILE ".ozymem"
$BinDir = Join-Path $OzyDir "bin"
$PyDir  = Join-Path $OzyDir "python\ozy-brain"

Write-Host "[1/4] Preparing installation directories..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path $PyDir) | Out-Null

Write-Host "[2/4] Building release binaries (cargo build --release)..." -ForegroundColor Yellow
try {
    cargo build --release
} catch {
    Write-Host "[!] Cargo release build failed. Ensure Rust is installed." -ForegroundColor Red
    exit 1
}

$ServerRelease = Join-Path $PSScriptRoot "target\release\ozymem-server.exe"
$CliRelease    = Join-Path $PSScriptRoot "target\release\ozymem.exe"

if (-not (Test-Path $ServerRelease) -or -not (Test-Path $CliRelease)) {
    Write-Host "[!] Release binaries not found in target\release." -ForegroundColor Red
    exit 1
}

Write-Host "[3/4] Installing executables and Ozy Brain worker..." -ForegroundColor Yellow
Copy-Item -Path $ServerRelease -Destination (Join-Path $BinDir "ozymem-server.exe") -Force
Copy-Item -Path $CliRelease    -Destination (Join-Path $BinDir "ozymem.exe") -Force

if (Test-Path (Join-Path $PSScriptRoot "python\ozy-brain")) {
    Copy-Item -Path (Join-Path $PSScriptRoot "python\ozy-brain") -Destination (Split-Path $PyDir) -Recurse -Force
}

Write-Host "[4/4] Configuring environment variables..." -ForegroundColor Yellow
[Environment]::SetEnvironmentVariable("OZY_BRAIN_PATH", $PyDir, "User")
$env:OZY_BRAIN_PATH = $PyDir

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinDir*") {
    $NewPath = "$UserPath;$BinDir"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host " -> Added $BinDir to User PATH." -ForegroundColor Green
} else {
    Write-Host " -> $BinDir is already in User PATH." -ForegroundColor Green
}

$ServerPathEscaped = (Join-Path $BinDir "ozymem-server.exe").Replace("\", "\\")

Write-Host "`n======================================================" -ForegroundColor Green
Write-Host "   Ozymem / Ozygram Installed Successfully!           " -ForegroundColor Green
Write-Host "======================================================" -ForegroundColor Green
Write-Host "`nAdd this configuration to your MCP settings (mcp_servers):`n" -ForegroundColor White

Write-Host "{`n  `"ozygram`": {`n    `"command`": `"$ServerPathEscaped`",`n    `"args`": []`n  }`n}" -ForegroundColor Cyan

Write-Host "`nTest installation by running in a new terminal:" -ForegroundColor Yellow
Write-Host "  ozymem list" -ForegroundColor White
Write-Host "  ozymem q arch`n" -ForegroundColor White
