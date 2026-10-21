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
if (Test-Path "target\release\ozymem-server.exe") {
    Move-Item -Force "target\release\ozymem-server.exe" "target\release\ozymem-server.exe.running" -ErrorAction SilentlyContinue
}
if (Test-Path "target\release\ozymem.exe") {
    Move-Item -Force "target\release\ozymem.exe" "target\release\ozymem.exe.running" -ErrorAction SilentlyContinue
}
try {
    cargo build --release
} catch {
    Write-Host "[!] Cargo release build failed. Ensure Rust is installed." -ForegroundColor Red
    exit 1
}

$ServerRelease  = Join-Path $PSScriptRoot "target\release\ozymem-server.exe"
$CliRelease     = Join-Path $PSScriptRoot "target\release\ozymem.exe"
$OzygramRelease = Join-Path $PSScriptRoot "target\release\ozygram.exe"
$BrainRelease   = Join-Path $PSScriptRoot "target\release\ozy-brain.exe"

if (-not (Test-Path $ServerRelease) -or -not (Test-Path $CliRelease)) {
    Write-Host "[!] Release binaries not found in target\release." -ForegroundColor Red
    exit 1
}

Write-Host "[3/4] Installing executables and Ozy Brain worker..." -ForegroundColor Yellow
$DestServer  = Join-Path $BinDir "ozymem-server.exe"
$DestCli     = Join-Path $BinDir "ozymem.exe"
$DestOzygram = Join-Path $BinDir "ozygram.exe"
$DestBrain   = Join-Path $BinDir "ozy-brain.exe"

if (Test-Path $DestServer) {
    Move-Item -Force $DestServer "$DestServer.old" -ErrorAction SilentlyContinue
}
if (Test-Path $DestCli) {
    Move-Item -Force $DestCli "$DestCli.old" -ErrorAction SilentlyContinue
}
if (Test-Path $DestOzygram) {
    Move-Item -Force $DestOzygram "$DestOzygram.old" -ErrorAction SilentlyContinue
}
if (Test-Path $DestBrain) {
    Move-Item -Force $DestBrain "$DestBrain.old" -ErrorAction SilentlyContinue
}

Copy-Item -Path $ServerRelease -Destination $DestServer -Force
Copy-Item -Path $CliRelease    -Destination $DestCli -Force
if (Test-Path $OzygramRelease) {
    Copy-Item -Path $OzygramRelease -Destination $DestOzygram -Force
} else {
    Copy-Item -Path $CliRelease     -Destination $DestOzygram -Force
}
if (Test-Path $BrainRelease) {
    Copy-Item -Path $BrainRelease -Destination $DestBrain -Force
    Write-Host " -> Deployed standalone ozy-brain.exe (Zero-Python runtime bundled)." -ForegroundColor Green
}

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

Write-Host "`n======================================================" -ForegroundColor Green
Write-Host "   Ozymem / Ozygram Installed Successfully!           " -ForegroundColor Green
Write-Host "======================================================" -ForegroundColor Green

Write-Host "`n[Auto-Configuration] Scanning and auto-configuring installed AI clients..." -ForegroundColor Yellow
try {
    & $DestCli init --auto
} catch {
    Write-Host " -> Run 'ozymem init' to configure AI clients at any time." -ForegroundColor DarkGray
}

Write-Host "`nTest installation by running in a new terminal:" -ForegroundColor Yellow
Write-Host "  ozymem list" -ForegroundColor White
Write-Host "  ozymem q arch`n" -ForegroundColor White
