# Go Magic Desktop - Windows 构建脚本
# 使用方法：在 PowerShell 中以管理员身份运行

param(
    [switch]$SkipNSIS,
    [switch]$SkipMSI
)

$ErrorActionPreference = "Stop"

Write-Host "=== Go Magic Desktop Windows 构建脚本 ===" -ForegroundColor Cyan

# 检查管理员权限
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "警告: 建议以管理员身份运行此脚本" -ForegroundColor Yellow
}

# 1. 检查 Rust MSVC 工具链
Write-Host "`n[1/5] 检查 Rust 安装..." -ForegroundColor Green
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Host "正在安装 Rust..." -ForegroundColor Yellow
    Invoke-Expression "& {$(Invoke-WebRequest -Uri 'https://rustup.rs' -UseBasicParsing).Content}"
    refreshenv
}

rustc --version
cargo --version

# 检查 MSVC 工具链
$targets = cargo target list --installed
if ($targets -notmatch "x86_64-pc-windows-msvc") {
    Write-Host "添加 MSVC 目标..." -ForegroundColor Yellow
    rustup target add x86_64-pc-windows-msvc
}

# 2. 检查 Node.js
Write-Host "`n[2/5] 检查 Node.js 安装..." -ForegroundColor Green
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host "正在安装 Node.js..." -ForegroundColor Yellow
    winget install -e --id OpenJS.NodeJS.LTS --silent
    refreshenv
}

node --version
npm --version

# 3. 克隆/更新代码
Write-Host "`n[3/5] 准备源代码..." -ForegroundColor Green
$projectDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $projectDir

if (Test-Path ".git") {
    Write-Host "拉取最新代码..." -ForegroundColor Yellow
    git pull origin main
} else {
    Write-Host "克隆 go-magic..." -ForegroundColor Yellow
    if (-not (Test-Path "../go-magic")) {
        git clone https://github.com/magicwubiao/go-magic.git ../go-magic
    }
}

# 4. 安装依赖
Write-Host "`n[4/5] 安装依赖..." -ForegroundColor Green
npm install

# 5. 构建应用
Write-Host "`n[5/5] 构建应用..." -ForegroundColor Green

# 切换到 MSVC 工具链
rustup default stable-x86_64-pc-windows-msvc

# 构建 Tauri
npm run build

# 输出结果
Write-Host "`n=== 构建完成 ===" -ForegroundColor Cyan
Write-Host "`n构建产物位置:" -ForegroundColor Green

$bundleDir = Join-Path $projectDir "src-tauri\target\release\bundle"

if (Test-Path (Join-Path $bundleDir "nsis")) {
    Write-Host "NSIS 安装包:" (Get-ChildItem (Join-Path $bundleDir "nsis") -Filter "*.exe" | Select-Object -ExpandProperty FullName)
}

if (Test-Path (Join-Path $bundleDir "msi")) {
    Write-Host "MSI 安装包:" (Get-ChildItem (Join-Path $bundleDir "msi") -Filter "*.msi" | Select-Object -ExpandProperty FullName)
}

Write-Host "`n直接运行文件:" -ForegroundColor Yellow
Write-Host (Join-Path $projectDir "src-tauri\target\release\go-magic-desktop.exe")

Write-Host "`n按任意键退出..." -ForegroundColor Gray
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
