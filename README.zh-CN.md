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
│  ✓ 窗口状态持久化（位置和大小记忆）                          │
│  ✓ 高 DPI 感知的窗口定位                                   │
│  ✓ 外部链接在系统浏览器中打开                               │
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
- **窗口即时出现**: 健康检查在后台线程执行，不再阻塞启动（此前最多阻塞 60 秒，且期间没有任何窗口）
- **健康检查**: 60 秒就绪检测预算，并以限流方式推送 `backend-status` 进度事件
- **可靠的进程管理**: 持续读取后端 stdout/stderr（避免管道死锁）、退出时回收子进程、Windows 上结束整个进程树
- **窗口状态持久化**: 跨会话记忆窗口位置和大小
- **高 DPI 支持**: 显示缩放下正确的窗口定位和尺寸
- **外部链接处理**: 自动在系统浏览器中打开非本地链接（仅 http/https）
- **后端重启**: 无需关闭应用、也不阻塞 UI 线程即可重启后端
- **应用信息查询**: 运行时获取版本、git 提交、git 分支和构建元数据
- **日志系统**: 结构化日志同时写入 stdout **和**日志文件，覆盖外壳与后端
- **安全加固**: 最小权限 capabilities、严格 CSP、二进制不链接任何 TLS 栈、release 构建不含 devtools
- **已有测试**: 纯函数单元测试；CI 强制 `cargo fmt --check`，clippy 仍为 advisory

## 使用教程

### 快速开始

1. **下载安装** — 从 [Releases](https://github.com/magicwubiao/go-magic-desktop/releases) 页面获取对应平台的安装包：
   - Windows：`.exe`（NSIS）或 `.msi`
   - macOS：`.dmg`（Intel 与 Apple Silicon）
   - Linux：`.AppImage` 或 `.deb`

2. **启动应用** — 打开应用（窗口标题为 **“Go Magic”**）。桌面端会自动完成以下工作：
   - 在打包资源中定位 `go-magic` 后端可执行文件；
   - 自动选择可用端口（默认 `5000`，依次回退到 `5001` / `5002` / … / `3000`）；
   - 立即显示窗口，同时启动后端并最多等待 60 秒进行健康检查；
   - 后端就绪后立即在窗口内加载 Go Magic 的 Web 界面。

3. **开始使用** — 所有操作都在窗口内完成，即标准的 Go Magic Web 界面，无需额外配置。

### 后端管理

- 后端进程由应用**自动管理**，随应用启动，关闭窗口时自动终止。
- **界面内重启**：若后端无响应，可通过应用内的重启操作（调用 `restart_backend_cmd`）重启后端，无需退出应用。
- **应用信息**：运行时可通过 `get_app_info` 查询版本、git 提交与构建元数据。

### 链接与导航

- 指向 `127.0.0.1` / `localhost` 的链接在应用内打开；
- 任何**外部链接**都会自动在**系统默认浏览器**中打开。

### 窗口与布局

- 默认窗口大小为 1024×800（最小 800×600）。
- 窗口位置与大小**跨会话记忆**，下次启动自动恢复，并适配高分屏（Hi-DPI）。

### 退出应用

- 关闭窗口即退出应用，应用会保存窗口状态并优雅终止后端进程。

### 日志与排查

日志位置：
- **Windows**：`%APPDATA%/go-magic-desktop/logs/`
- **macOS**：`~/Library/Logs/go-magic-desktop/`
- **Linux**：`~/.local/share/go-magic-desktop/logs/`

若应用启动失败，请参阅下方的[常见问题](#常见问题)。

## 项目结构

```
go-magic-desktop/
├── src-tauri/                  # Rust 外壳 + 后端管控
│   ├── src/main.rs             # 主程序入口（进程管理、窗口）
│   ├── build.rs                # 嵌入版本/提交/构建时间等元数据
│   ├── Cargo.toml              # Rust 依赖
│   ├── tauri.conf.json         # Tauri 配置（窗口、CSP、打包）
│   ├── capabilities/           # 最小权限声明
│   ├── icons/                  # 应用图标
│   └── resources/              # 打包用的后端二进制（构建时生成）
├── scripts/sync-version.mjs    # 将 git tag 写入所有版本字段
├── .github/workflows/build.yml # 多平台 CI + 发布
└── package.json                # Node.js 配置
```

## 前置要求

### 必需软件

1. **Rust** (1.77.2+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Node.js** (18+)

3. **Tauri CLI**
   ```bash
   npm install -g @tauri-apps/cli
   ```

4. **Go** (1.22+) — 用于构建后端
   ```bash
   # https://go.dev/dl/
   ```

### 系统依赖

- **Windows**: Microsoft Visual Studio C++ Build Tools, WebView2 Runtime
- **macOS**: Xcode Command Line Tools
- **Linux**:
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf
  ```
  无需 OpenSSL 开发包：外壳及其依赖的 Tauri 组件都不链接 TLS 后端，`openssl-sys`、
  `native-tls`、`rustls` 均不会进入构建。

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

### 测试、格式化与静态检查

```bash
npm test               # cargo test —— 纯函数单元测试
npm run fmt            # cargo fmt
npm run clippy         # cargo clippy --all-targets -- -D warnings
npm run version:sync   # 将当前 git tag 写入 package.json / Cargo.toml / tauri.conf.json
```

CI 在独立的 lint 任务中强制 `cargo fmt --check`；`cargo clippy` 仍为 advisory ——
代码库 clippy 清理干净后，可在 `.github/workflows/build.yml` 中移除
`continue-on-error`。

## 构建

### 快速构建

```bash
# 构建前端并打包桌面应用
npm run build

# 或构建指定平台
npm run build:linux
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

Tauri 应用不适合交叉编译，各平台需在对应 runner（或本机）上通过 npm 脚本构建：

```bash
npm run build:windows    # x86_64-pc-windows-msvc
npm run build:macos      # x86_64-apple-darwin
npm run build:macos-arm  # aarch64-apple-darwin
npm run build:linux      # x86_64-unknown-linux-gnu
```

CI 对四个目标执行同样的流程，推 tag 即可。

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
3. **健康检查**: 等待后端就绪（60 秒预算，失败则弹出错误并退出）
4. **加载界面**: WebView 加载 `http://127.0.0.1:<PORT>/`
5. **窗口管理**: 显示主窗口并聚焦
6. **优雅关闭**: 窗口关闭时终止后端进程

### 端口优先级

自动选择可用端口：5000 → 5001 → 5002 → 5003 → 5004 → 8080 → 3000

### Tauri 命令

| 命令 | 说明 |
|------|------|
| `get_backend_port` | 获取后端运行端口 |
| `restart_backend_cmd` | 重启后端进程 |
| `get_app_info` | 获取应用版本、git 提交、构建时间和配置 |
| `check_backend_health_cmd` | 检查后端是否健康 |

## 配置

### Tauri 配置

修改 `tauri.conf.json` 调整：
- 窗口大小和标题
- 权限设置
- 打包选项

### 环境变量

| 变量 | 说明 |
|------|------|
| `GOMAGIC_PORT` | 后端监听端口（同时以 `--port` 传入） |
| `RUST_BACKTRACE` | 仅在 **debug** 构建中为后端设为 `1` |

构建期元数据（`APP_VERSION`、`GIT_COMMIT`、`GIT_BRANCH`、`BUILD_TIME`、
`BUILD_PROFILE`）由 `src-tauri/build.rs` 注入，并通过 `get_app_info` 暴露。

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

- **CSP**: 脚本/样式/连接目标仅限应用自身与本地后端，并附
  `object-src 'none'`、`base-uri 'self'`、`frame-ancestors 'none'`；
  不再允许 `'unsafe-eval'`（捆绑 UI 的内联脚本/样式仍需要 `'unsafe-inline'`，
  `img-src` 还额外允许 `https:` 与 `data:` 图片）。
- **端口绑定**: 后端只绑定 `127.0.0.1`。
- **权限**: `src-tauri/capabilities/default.json` 仅授予 `core:default`，且仅对
  本地内容生效。Go Magic UI 由捆绑后端通过 `http://127.0.0.1:<port>` 提供，
  Tauri 会将其视为**远程**内容——因此该 UI 刻意**没有**任何 Tauri 命令或插件权限。
  若该来源确实需要 IPC，请显式添加 `remote` capability，而不是放宽默认 capability。
- **release 不含 devtools**: `devtools` cargo feature 默认关闭，且窗口构建时在
  非 `debug_assertions` 下显式禁用 devtools。
- **TLS**: 外壳自身不引入 HTTP 客户端 —— 唯一的请求（对 `127.0.0.1` 的 `GET /health`）
  走裸 socket。`Cargo.lock` 中不含任何 TLS crate（`openssl`、`native-tls`、`rustls`、`ring`
  全部缺失），桌面构建也不会引入 `reqwest`/`hyper`：tauri 仅在非桌面目标上需要它们。
- **外部链接**: 仅将 `http`/`https` 交给系统处理。

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

- **lint**: 全量 `cargo fmt --check`（阻断式）
- **build**: 多平台构建（Windows / macOS-x64 / macOS-arm64 / Linux），启用
  npm 与 cargo 缓存，在 Linux 分支运行单元测试与 advisory clippy，并在某平台
  没有产出安装包时直接失败
- **release**: 标签推送时自动发布 GitHub Release 并附带校验和

后端从 `magicwubiao/go-magic` 的 `GO_MAGIC_REF`（workflow 输入或仓库变量，
默认 `main`）构建——建议固定为 tag 或 commit SHA 以保证可复现。

当存在 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、
`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`
secrets 时，macOS 构建会自动签名与公证；不存在时会静默跳过。

## 版本管理

版本号**完全由 git 标签派生**——不手动维护任何配置文件中的版本字段。
`package.json`、`Cargo.toml` 和 `tauri.conf.json` 均保持占位符 `0.0.0`。

构建时，`build.rs` 运行 `git describe --tags` 将真实版本（以及 git 提交、分支、
构建时间、构建配置）嵌入二进制文件。CI 标签推送时会执行
`scripts/sync-version.mjs <tag>`，将同一版本写入 `package.json`、
`src-tauri/Cargo.toml` 与 `src-tauri/tauri.conf.json`，使安装包也携带正确版本。
本地可用同一脚本预览效果。

### 发布流程

```bash
# 只需打标签并推送即可
git tag v1.0.0
git push origin v1.0.0
```

CI 将自动构建 Windows、macOS（x64 + arm64）、Linux 版本，并创建附带校验和的
GitHub Release。

### 本地构建

没有标签的本地构建会生成版本 `0.0.0-dev`。要使用特定版本进行本地构建，先创建标签：

```bash
git tag v0.5.1
npm run build
```

## 许可证

MIT License

## 贡献

欢迎提交 Issue 和 Pull Request！