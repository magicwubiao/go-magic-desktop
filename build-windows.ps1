# Go Magic Desktop - Windows 构建脚本
# 使用方式: .\build-windows.ps1 [参数]

param(
    [switch]$Clean,
    [switch]$Debug,
    [switch]$SkipFrontend
)

$ErrorActionPreference = "Stop"

# 颜色定义
function Write-Info { Write-Host "[INFO] $args" -ForegroundColor Cyan }
function Write-Success { Write-Host "[SUCCESS] $args" -ForegroundColor Green }
function Write-Warn { Write-Host "[WARN] $args" -ForegroundColor Yellow }
function Write-Err { Write-Host "[ERROR] $args" -ForegroundColor Red }

Write-Host "========================================" -ForegroundColor Magenta
Write-Host "Go Magic Desktop - Windows 构建" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta

# 切换到脚本目录
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

# 清理
if ($Clean) {
    Write-Info "清理构建产物..."
    if (Test-Path "src-tauri/target") {
        Remove-Item -Recurse -Force "src-tauri/target"
    }
    Write-Success "清理完成"
}

# 检查依赖
Write-Info "检查构建依赖..."

# 检查 Rust
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Err "Rust 未安装"
    Write-Host "请运行: winget install Rustlang.Rust.MSVC"
    exit 1
}

# 检查 Node.js
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Err "Node.js 未安装"
    Write-Host "请运行: winget install OpenJS.NodeJS.LTS"
    exit 1
}

# 检查 Tauri CLI
if (-not (Get-Command tauri -ErrorAction SilentlyContinue)) {
    Write-Warn "Tauri CLI 未安装，正在安装..."
    npm install -g @tauri-apps/cli
}

Write-Success "依赖检查完成"

# 构建前端
if (-not $SkipFrontend) {
    Write-Info "构建前端..."

    $GoMagicWeb = Join-Path $ScriptDir "..\go-magic\web"
    if (Test-Path $GoMagicWeb) {
        Push-Location $GoMagicWeb
        npm install
        npm run build
        Pop-Location
        Write-Success "前端构建完成"
    } else {
        Write-Warn "go-magic/web 目录不存在，请确保 web-dist 存在"
    }
}

# 检查 WebView2
Write-Info "检查 WebView2 运行时..."
$WebView2Path = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
if (Test-Path $WebView2Path) {
    $Version = (Get-ItemProperty -Path $WebView2Path -Name pv -ErrorAction SilentlyContinue).pv
    Write-Success "WebView2 已安装 (版本: $Version)"
} else {
    Write-Warn "WebView2 未检测到"
    Write-Host "下载地址: https://developer.microsoft.com/en-us/microsoft-edge/webview2/"
}

# 构建 Tauri 应用
Write-Info "开始构建 Tauri 应用..."

$BuildArgs = @("build", "--target", "x86_64-pc-windows-msvc")
if ($Debug) {
    $BuildArgs += "--debug"
}

& tauri @BuildArgs

if ($LASTEXITCODE -eq 0) {
    Write-Success "构建完成！"

    # 显示输出
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Magenta
    Write-Host "构建输出:" -ForegroundColor Magenta
    Write-Host "========================================" -ForegroundColor Magenta

    $BundleDir = "src-tauri\target\release\bundle"

    if (Test-Path "$BundleDir\nsis") {
        Write-Host "NSIS 安装包:" -ForegroundColor Green
        Get-ChildItem "$BundleDir\nsis\*.exe" | ForEach-Object { Write-Host "  $($_.Name)" }
    }

    if (Test-Path "$BundleDir\msi") {
        Write-Host "MSI 安装包:" -ForegroundColor Green
        Get-ChildItem "$BundleDir\msi\*.msi" | ForEach-Object { Write-Host "  $($_.Name)" }
    }

    if (Test-Path "$BundleDir\exe") {
        Write-Host "独立可执行文件:" -ForegroundColor Green
        Get-ChildItem "$BundleDir\exe\*.exe" | ForEach-Object { Write-Host "  $($_.Name)" }
    }

    Write-Host ""
} else {
    Write-Err "构建失败"
    exit 1
}
