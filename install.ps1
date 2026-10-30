# Ozygram / Ozymem Automatic Release Installer for Windows
# Run via: powershell -ExecutionPolicy Bypass -File .\install.ps1
# Or one-liner: irm https://raw.githubusercontent.com/Xangel0s/ozygram/main/install.ps1 | iex

param(
    [switch]$FromSource = $false
)

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

$InstalledFromRelease = $false

# If not explicitly compiling from source, attempt fast precompiled release download
if (-not $FromSource -and -not (Test-Path "target\release\ozymem.exe")) {
    Write-Host "[2/4] Downloading precompiled release v0.2.2 from GitHub..." -ForegroundColor Yellow
    $ReleaseZip = Join-Path $env:TEMP "ozymem-windows-x64.zip"
    $TempExtract = Join-Path $env:TEMP "ozymem-install-extract"
    $DownloadUrl = "https://github.com/Xangel0s/ozygram/releases/latest/download/ozymem-windows-x64.zip"
    $Sha256Url = "https://github.com/Xangel0s/ozygram/releases/latest/download/SHA256SUMS"

    try {
        Write-Host " -> Fetching $DownloadUrl..."
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $ReleaseZip -UseBasicParsing
        
        # Verify SHA256 checksum if SHA256SUMS is available
        $ShaFile = Join-Path $env:TEMP "SHA256SUMS"
        try {
            Invoke-WebRequest -Uri $Sha256Url -OutFile $ShaFile -UseBasicParsing -TimeoutSec 5
            if (Test-Path $ShaFile) {
                $ExpectedHash = (Get-Content $ShaFile | Select-String "ozymem-windows-x64.zip").ToString().Split(" ")[0].Trim()
                if ($ExpectedHash) {
                    $ActualHash = (Get-FileHash -Path $ReleaseZip -Algorithm SHA256).Hash.ToLower()
                    if ($ActualHash -ne $ExpectedHash.ToLower()) {
                        throw "SHA256 checksum mismatch (expected $ExpectedHash, got $ActualHash)"
                    }
                    Write-Host " [STATUS: VERIFIED] SHA256 checksum verified ($ActualHash)." -ForegroundColor Green
                }
            }
        } catch {
            Write-Host " [WARN] SHA256 verification skipped or unverified: $($_.Exception.Message)" -ForegroundColor DarkGray
        }
        
        if (Test-Path $TempExtract) {
            Remove-Item -Recurse -Force $TempExtract -ErrorAction SilentlyContinue
        }
        Expand-Archive -Path $ReleaseZip -DestinationPath $TempExtract -Force
        
        # Copy binaries safely
        foreach ($bin in @("ozymem-server.exe", "ozymem.exe", "ozygram.exe", "ozy-brain.exe")) {
            $src = Join-Path $TempExtract $bin
            $dst = Join-Path $BinDir $bin
            if (Test-Path $src) {
                if (Test-Path $dst) {
                    Move-Item -Force $dst "$dst.old" -ErrorAction SilentlyContinue
                }
                Copy-Item -Path $src -Destination $dst -Force
            }
        }
        
        Remove-Item -Force $ReleaseZip -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $TempExtract -ErrorAction SilentlyContinue
        $InstalledFromRelease = $true
        Write-Host " -> Precompiled binaries deployed successfully to $BinDir" -ForegroundColor Green
    } catch {
        Write-Host " [WARN] Could not download GitHub release ($($_).Exception.Message), falling back to source build..." -ForegroundColor Yellow
    }
}

if (-not $InstalledFromRelease) {
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

    $Root = if ($PSScriptRoot) { $PSScriptRoot } else { "." }
    $ServerRelease  = Join-Path $Root "target\release\ozymem-server.exe"
    $CliRelease     = Join-Path $Root "target\release\ozymem.exe"
    $OzygramRelease = Join-Path $Root "target\release\ozygram.exe"
    $BrainRelease   = Join-Path $Root "target\release\ozy-brain.exe"

    $PrimaryRelease = if (Test-Path $OzygramRelease) { $OzygramRelease } elseif (Test-Path $CliRelease) { $CliRelease } else { $null }
    if (-not $PrimaryRelease) {
        Write-Host "[!] Release binary not found in target\release." -ForegroundColor Red
        exit 1
    }

    Write-Host "[3/4] Installing executables and Ozy Brain worker..." -ForegroundColor Yellow
    $DestServer  = Join-Path $BinDir "ozymem-server.exe"
    $DestCli     = Join-Path $BinDir "ozymem.exe"
    $DestOzygram = Join-Path $BinDir "ozygram.exe"
    $DestBrain   = Join-Path $BinDir "ozy-brain.exe"

    if (Test-Path $DestServer) { Move-Item -Force $DestServer "$DestServer.old" -ErrorAction SilentlyContinue }
    if (Test-Path $DestCli) { Move-Item -Force $DestCli "$DestCli.old" -ErrorAction SilentlyContinue }
    if (Test-Path $DestOzygram) { Move-Item -Force $DestOzygram "$DestOzygram.old" -ErrorAction SilentlyContinue }
    if (Test-Path $DestBrain) { Move-Item -Force $DestBrain "$DestBrain.old" -ErrorAction SilentlyContinue }

    Copy-Item -Path $PrimaryRelease -Destination $DestOzygram -Force
    Copy-Item -Path $PrimaryRelease -Destination $DestCli -Force
    if (Test-Path $ServerRelease) {
        Copy-Item -Path $ServerRelease -Destination $DestServer -Force
    } else {
        Copy-Item -Path $PrimaryRelease -Destination $DestServer -Force
    }
    if (Test-Path $BrainRelease) {
        Copy-Item -Path $BrainRelease -Destination $DestBrain -Force
        Write-Host " -> Deployed standalone ozy-brain.exe (Zero-Python runtime bundled)." -ForegroundColor Green
    }
}

$Root = if ($PSScriptRoot) { $PSScriptRoot } else { "." }
if (Test-Path (Join-Path $Root "python\ozy-brain")) {
    Copy-Item -Path (Join-Path $Root "python\ozy-brain") -Destination (Split-Path $PyDir) -Recurse -Force
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

$DestCli = Join-Path $BinDir "ozymem.exe"
Write-Host "`n[Auto-Configuration] Scanning and auto-configuring installed AI clients..." -ForegroundColor Yellow
try {
    & $DestCli init --auto
} catch {
    Write-Host " -> Run 'ozymem init' to configure AI clients at any time." -ForegroundColor DarkGray
}

Write-Host "`nTest installation by running in a new terminal:" -ForegroundColor Yellow
Write-Host "  ozymem list" -ForegroundColor White
Write-Host "  ozymem q arch`n" -ForegroundColor White
