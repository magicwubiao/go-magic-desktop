# Go Magic Desktop - Windows Build Script
# Usage: .\build-windows.ps1 [options]

param(
    [switch]$Clean,
    [switch]$Debug,
    [switch]$SkipFrontend
)

$ErrorActionPreference = "Stop"

# Color definitions
function Write-Info { Write-Host "[INFO] $args" -ForegroundColor Cyan }
function Write-Success { Write-Host "[SUCCESS] $args" -ForegroundColor Green }
function Write-Warn { Write-Host "[WARN] $args" -ForegroundColor Yellow }
function Write-Err { Write-Host "[ERROR] $args" -ForegroundColor Red }

Write-Host "========================================" -ForegroundColor Magenta
Write-Host "Go Magic Desktop - Windows Build" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta

# Switch to script directory
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

# Clean
if ($Clean) {
    Write-Info "Cleaning build artifacts..."
    if (Test-Path "src-tauri/target") {
        Remove-Item -Recurse -Force "src-tauri/target"
    }
    Write-Success "Clean completed"
}

# Check dependencies
Write-Info "Checking build dependencies..."

# Check Rust
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Err "Rust not installed"
    Write-Host "Please run: winget install Rustlang.Rust.MSVC"
    exit 1
}

# Check Node.js
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Err "Node.js not installed"
    Write-Host "Please run: winget install OpenJS.NodeJS.LTS"
    exit 1
}

# Check Tauri CLI
if (-not (Get-Command tauri -ErrorAction SilentlyContinue)) {
    Write-Warn "Tauri CLI not installed, installing..."
    npm install -g @tauri-apps/cli
}

Write-Success "Dependency check completed"

# Build frontend
if (-not $SkipFrontend) {
    Write-Info "Building frontend..."

    $GoMagicWeb = Join-Path $ScriptDir "..\go-magic\web"
    if (Test-Path $GoMagicWeb) {
        Push-Location $GoMagicWeb
        npm install
        npm run build
        Pop-Location
        Write-Success "Frontend built successfully"
    } else {
        Write-Warn "go-magic/web directory not found, ensure web-dist exists"
    }
}

# Check WebView2
Write-Info "Checking WebView2 runtime..."
$WebView2Path = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
if (Test-Path $WebView2Path) {
    $Version = (Get-ItemProperty -Path $WebView2Path -Name pv -ErrorAction SilentlyContinue).pv
    Write-Success "WebView2 installed (version: $Version)"
} else {
    Write-Warn "WebView2 not detected"
    Write-Host "Download: https://developer.microsoft.com/en-us/microsoft-edge/webview2/"
}

# Build Tauri app
Write-Info "Starting Tauri build..."

$BuildArgs = @("build", "--target", "x86_64-pc-windows-msvc")
if ($Debug) {
    $BuildArgs += "--debug"
}

& tauri @BuildArgs

if ($LASTEXITCODE -eq 0) {
    Write-Success "Build completed!"

    # Show output
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Magenta
    Write-Host "Build Output:" -ForegroundColor Magenta
    Write-Host "========================================" -ForegroundColor Magenta

    $BundleDir = "src-tauri\target\release\bundle"

    if (Test-Path "$BundleDir\nsis") {
        Write-Host "NSIS Installer:" -ForegroundColor Green
        Get-ChildItem "$BundleDir\nsis\*.exe" | ForEach-Object { Write-Host "  $($_.Name)" }
    }

    if (Test-Path "$BundleDir\msi") {
        Write-Host "MSI Installer:" -ForegroundColor Green
        Get-ChildItem "$BundleDir\msi\*.msi" | ForEach-Object { Write-Host "  $($_.Name)" }
    }

    if (Test-Path "$BundleDir\exe") {
        Write-Host "Standalone Executable:" -ForegroundColor Green
        Get-ChildItem "$BundleDir\exe\*.exe" | ForEach-Object { Write-Host "  $($_.Name)" }
    }

    Write-Host ""
} else {
    Write-Err "Build failed"
    exit 1
}