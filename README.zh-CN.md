# Go Magic Desktop (Tauri)

[English](README.md) | [中文](README.zh-CN.md)

Tauri 桌面端应用，将 Go Magic 打包为跨平台桌面应用。

## 架构说明

```
┌────────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                        │
│                                                            │
│  ┌────────────────┐     ┌─────────────────────────────┐   │
│  │   WebView      │     │    go-magic 后端进程        │   │
│  │   (UI)         │◀───▶│    - HTTP 服务 (port)      │   │
│  │                │     │    - API 处理              │   │
│  │   localhost    │     │    - 独立运行              │   │
│  └────────────────┘     └─────────────────────────────┘   │
│                                                            │
│  特点：                                                    │
│  ✓ CLI 和桌面版共用同一后端                                 │
│  ✓ 无需修改 go-magic 源码                                   │
│  ✓ 版本升级只需替换二进制                                   │
└────────────────────────────────────────────────────────────┘
```

**为什么选择进程分离？**
- go-magic 是独立 Go 程序，非库
- CLI 和桌面版完全共享后端
- 维护简单，版本同步容易

## 功能特性

- **跨平台支持**: Windows、macOS、Linux
- **嵌入式后端**: Go Magic 后端自动启动和管理
- **自动端口选择**: 智能选择可用端口，避免冲突
- **健康检查**: 60秒超时检测后端就绪状态
- **日志系统**: 结构化日志便于问题排查
- **安全策略**: CSP 防护、权限控制

## 项目结构

```
go-magic-desktop/
├── src-tauri/          # Rust 后端代码
│   ├── src/
│   │   └── main.rs     # 主程序入口
│   ├── Cargo.toml      # Rust 依赖
│   ├── tauri.conf.json # Tauri 配置
│   └── resources/      # 打包资源
├── icons/              # 应用图标
├── build-all.sh        # 多平台构建脚本
├── build-windows.ps1   # Windows 构建脚本
└── package.json        # Node.js 配置
```

## 前置要求

### 必需软件

1. **Rust** (1.75+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Node.js** (18+)

3. **Tauri CLI**
   ```bash
   npm install -g @tauri-apps/cli
   ```

4. **Go** (1.26+) - 用于构建后端
   ```bash
   go install golang.org/dl/go1.21@latest
   ```

### 系统依赖

- **Windows**: Microsoft Visual Studio C++ Build Tools, WebView2 Runtime
- **macOS**: Xcode Command Line Tools
- **Linux**:
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libssl3 libgtk-3-dev
  ```

## 开发

```bash
# 进入项目目录
cd go-magic-desktop

# 安装依赖
npm install

# 克隆 Go Magic 主仓库（在同一父目录）
cd ../ && git clone https://github.com/magicwubiao/go-magic.git

# 返回桌面应用目录
cd go-magic-desktop

# 构建 Go Magic 后端
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# 开发模式
npm run dev
```

## 构建

### 快速构建

```bash
# 构建前端并打包桌面应用
npm run build

# 或使用构建脚本
./build-all.sh
```

### 分步构建

```bash
# 1. 构建前端
cd ../go-magic/web && npm run build

# 2. 构建后端
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# 3. 打包桌面应用
cd ../go-magic-desktop && tauri build
```

### 多平台构建

```bash
# 所有平台
./build-all.sh all

# Windows
./build-all.sh windows

# macOS (Intel)
./build-all.sh macos

# macOS (Apple Silicon)
./build-all.sh macos-arm

# Linux
./build-all.sh linux
```

## 打包输出

构建完成后，安装包位于：

| 平台 | 位置 |
|------|------|
| Windows NSIS | `src-tauri/target/release/bundle/nsis/*.exe` |
| Windows MSI | `src-tauri/target/release/bundle/msi/*.msi` |
| macOS | `src-tauri/target/release/bundle/dmg/*.dmg` |
| Linux AppImage | `src-tauri/target/release/bundle/appimage/*.AppImage` |
| Linux DEB | `src-tauri/target/release/bundle/deb/*.deb` |

## 与 Go Magic 集成

Tauri 应用自动完成以下工作：

1. **检测后端**: 在 resources 目录查找 go-magic 可执行文件
2. **启动后端**: 自动启动 `go-magic server --port <PORT>`
3. **健康检查**: 等待后端就绪（60秒超时）
4. **加载界面**: WebView 加载 `http://127.0.0.1:<PORT>/`
5. **窗口管理**: 显示主窗口并聚焦
6. **优雅关闭**: 窗口关闭时终止后端进程

### 端口优先级

自动选择可用端口：5000 → 5001 → 5002 → 5003 → 5004 → 8080 → 3000

## 配置

### Tauri 配置

修改 `tauri.conf.json` 调整：
- 窗口大小和标题
- 权限设置
- 打包选项

### 环境变量

| 变量 | 说明 |
|------|------|
| `GOMAGIC_PORT` | 后端监听端口 |
| `RUST_BACKTRACE` | Rust 堆栈跟踪级别 |

## 架构说明

### 进程管理

```
┌─────────────────────────────────────┐
│         Tauri Main Process          │
│  ┌─────────────────────────────┐   │
│  │   Rust Backend Manager      │   │
│  │  - Process spawn/kill      │   │
│  │  - Health check            │   │
│  │  - Port allocation         │   │
│  └─────────────┬───────────────┘   │
│                │                    │
│  ┌─────────────▼───────────────┐   │
│  │   go-magic Backend Process  │   │
│  │   (独立子进程)              │   │
│  └─────────────┬───────────────┘   │
│                │                    │
│  ┌─────────────▼───────────────┐   │
│  │   WebView (用户界面)        │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
```

### 安全策略

- **CSP**: 限制脚本来源和连接目标
- **端口绑定**: 仅允许 localhost 访问
- **权限控制**: 仅允许必要的系统操作

## 日志

日志位置：
- **Windows**: `%APPDATA%/go-magic-desktop/logs/`
- **macOS**: `~/Library/Logs/go-magic-desktop/`
- **Linux**: `~/.local/share/go-magic-desktop/logs/`

日志文件：
- `go-magic.log`: 应用主日志
- `go-magic.log.0`: 轮转日志

## 常见问题

### WebView2 缺失 (Windows)

```powershell
winget install Microsoft.WebView2
```

### 后端启动失败

1. 检查 `src-tauri/resources/go-magic.exe` 是否存在
2. 查看日志文件中的错误信息
3. 验证端口是否被占用

### macOS 安全提示

系统偏好设置 → 安全性与隐私 → 允许运行

## 持续集成

使用 GitHub Actions 自动构建：

- **lint**: 代码质量检查
- **build-desktop**: 多平台桌面应用构建
- **build-go-cli**: Go CLI 多平台构建
- **create-release**: 自动化发布

## 许可证

MIT License

## 贡献

欢迎提交 Issue 和 Pull Request！
