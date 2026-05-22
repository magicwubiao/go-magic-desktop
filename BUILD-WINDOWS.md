# Go Magic Desktop - Windows 构建说明

## 问题说明
在 Linux 环境下交叉编译的 Windows Tauri 应用可能存在 WebView2Loader.dll 链接问题。

## 解决方案

### 方案 1：Windows 本地构建（推荐）

#### 1.1 安装依赖

**安装 Rust（使用 MSVC 工具链）：**
```powershell
# 使用管理员权限打开 PowerShell
winget install Rustlang.Rust.MSVC
# 或从 https://rustup.rs 安装，选择 MSVC 工具链
```

**安装 WebView2 运行时：**
- 下载地址：https://developer.microsoft.com/en-us/microsoft-edge/webview2/
- 选择 **Evergreen Standalone Installer** (x64)

**安装 Node.js：**
```powershell
winget install OpenJS.NodeJS.LTS
```

#### 1.2 克隆代码
```powershell
git clone https://github.com/magicwubiao/go-magic.git
git clone https://github.com/magicwubiao/go-magic-desktop.git
```

#### 1.3 构建应用
```powershell
cd go-magic-desktop

# 安装 npm 依赖
npm install

# 构建 Windows 应用
npm run build
```

构建产物将位于：
- `src-tauri/target/release/bundle/nsis/` - NSIS 安装包
- `src-tauri/target/release/bundle/msi/` - MSI 安装包

### 方案 2：使用 GitHub Actions（自动构建）

该仓库可以配置 GitHub Actions 来自动构建 Windows 版本。

### 方案 3：使用 Go CLI 版本

如果桌面应用有问题，可以先使用 Go CLI 版本：
```
go-magic-windows-amd64.exe
```

## 验证 WebView2 安装

在 Windows 上检查 WebView2 是否正确安装：
1. 按 `Win + R`，输入 `regedit`
2. 导航到 `HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`
3. 确认 `pv` 值存在且版本号正确

或者在 PowerShell 中运行：
```powershell
Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue | Select-Object pv
```

## 常见问题

### Q: WebView2Loader.dll 仍然找不到？
A: 确保使用 Windows 本地 MSVC 工具链构建，而不是交叉编译。

### Q: 程序启动后闪退？
A: 检查事件查看器中的应用程序日志，查看具体错误。

### Q: WebView2 版本过旧？
A: Windows 7/8 可能需要手动安装 WebView2，Windows 10/11 通常已预装。
