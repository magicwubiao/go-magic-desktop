# Go Magic Desktop

> **基于 Tauri 的 [Go Magic](https://github.com/magicwubiao/go-magic) 桌面端外壳。**

[English](README.md) | [中文](README.zh-CN.md)

Go Magic Desktop 将 Go Magic Web 应用封装为原生、跨平台的桌面应用。它捆绑
`go-magic` 后端，将其作为子进程托管，并在轻量级的 Tauri WebView 中渲染其 Web
界面——无需浏览器标签页、无需手动搭建服务器、无需任何额外配置。

---

## 目录

- [为什么做桌面版？](#为什么做桌面版)
- [功能亮点](#功能亮点)
- [快速开始](#快速开始)
- [使用应用](#使用应用)
- [项目结构](#项目结构)
- [前置要求](#前置要求)
- [开发](#开发)
- [构建与打包](#构建与打包)
- [工作原理](#工作原理)
- [配置](#配置)
- [更新检查](#更新检查)
- [安全模型](#安全模型)
- [日志与排查](#日志与排查)
- [CI/CD 与发布](#cicd-与发布)
- [版本管理](#版本管理)
- [许可证与贡献](#许可证与贡献)

---

## 为什么做桌面版？

`go-magic` 是独立的 Go 程序，而非库。与其重写其后端或 fork 其源码，本项目选择
把它当作**黑盒**并以子进程方式托管。这一设计带来三点好处：

- **共享后端** —— CLI 与桌面版运行的是完全相同的代码。
- **零源码改动** —— 从不修改 go-magic 本身。
- **升级轻松** —— 替换更新的二进制即可完成。

## 功能特点

| 领域 | 能力 |
|------|------|
| **跨平台** | Windows · macOS（Intel 与 Apple Silicon）· Linux |
| **嵌入式后端** | go-magic 随应用自动启动、托管、关闭 |
| **窗口即时出现** | 界面立即显示；健康检查在后台线程执行（不再有 60 秒启动卡顿）|
| **智能端口** | 自动选择端口，避免冲突 |
| **可靠的进程管理** | 持续读取 stdout/stderr（避免管道死锁）、退出时先优雅关闭再强制结束、回收子进程、Windows 上结束整个进程树 |
| **状态持久化** | 跨会话记忆窗口位置、大小与最大化状态 |
| **高 DPI 感知** | 显示缩放下正确的窗口尺寸与定位 |
| **外部链接** | `http`/`https` 链接在系统浏览器中打开；本地链接留在应用内 |
| **应用内重启** | 无需关闭应用、也不阻塞 UI 即可重启后端 |
| **运行时应用信息** | 版本、git 提交、分支与构建元数据随时可取 |
| **结构化日志** | 日志同时写入 stdout **和**文件，覆盖外壳与后端 |
| **安全加固** | 最小权限 capabilities、严格 CSP、不链接 TLS、release 不含 devtools |

---

## 快速开始

### 1. 下载

从 [Releases](https://github.com/magicwubiao/go-magic-desktop/releases) 页面获取
对应平台的安装包：

| 平台 | 安装包 |
| --- | --- |
| Windows | `.exe`（NSIS）|
| macOS | `.dmg`（Intel 与 Apple Silicon） |
| Linux | `.AppImage` · `.deb` |

### 2. 启动

打开应用（窗口标题：**"Go Magic"**）。启动时会自动完成以下工作：

1. 在打包资源中定位 `go-magic` 后端可执行文件。
2. 选择可用端口（优先 `5000 → 5001 → 5002`，否则随机探测高位临时端口）。
3. **立即**显示窗口，同时后台启动后端（健康检查预算：最多 60 秒）。
4. 后端就绪后立即在窗口内加载 Go Magic 的 Web 界面。

### 3. 开始使用

所有操作都在窗口内完成——即标准的 Go Magic Web 界面，无需任何配置。

---

## 使用应用

### 后端管理

- 后端进程由应用**全权管理**——随应用启动，关闭窗口时自动终止。
- **界面内重启**：若后端无响应，可通过应用内的重启操作（`restart_backend_cmd`）
  重启，无需退出。
- **应用信息**：随时通过 `get_app_info` 查询版本、git 提交与构建元数据。

### 链接与导航

- 指向 `127.0.0.1` / `localhost` 的链接在**应用内**打开。
- 任何**外部**链接都会在**系统默认浏览器**中打开。

### 窗口与布局

- 默认大小为 **1024×800**（最小 **800×600**）。
- 位置与大小**跨会话记忆**，下次启动自动恢复，并适配高分屏（Hi-DPI）。
- **最大化**状态会被正确恢复：只记忆「已最大化」标记，底层几何尺寸仍保留最大化
  之前的值。（若直接记忆最大化后的几何值，下次会以「普通窗口」还原一个屏幕大小的
  矩形，因边框存在而差几个像素无法真正铺满。）

### 退出应用

关闭窗口即退出应用——它会保存窗口状态并优雅终止后端进程。

---

## 项目结构

```
go-magic-desktop/
├── src-tauri/                  # Rust 外壳 + 后端管控
│   ├── src/main.rs             # 入口（builder、菜单、IPC 命令）
│   ├── src/backend.rs          # 后端进程的启动 / 监督 / 重启 / 停止
│   ├── src/window_state.rs     # 窗口几何状态持久化
│   ├── src/update.rs           # 隐藏更新检查窗口 + 弹窗流程
│   ├── src/locales.rs          # 按 OS 语言的壳层文案表
│   ├── src/tests.rs            # Rust 单元测试
│   ├── build.rs                # 嵌入版本 / 提交 / 构建时间元数据
│   ├── Cargo.toml              # Rust 依赖
│   ├── tauri.conf.json         # Tauri 配置（窗口、CSP、打包）
│   ├── capabilities/           # 最小权限声明
│   ├── icons/                  # 应用图标
│   └── resources/              # 打包用的后端二进制（构建时生成）
├── scripts/sync-version.mjs    # 将 git tag 写入所有版本字段
├── .github/workflows/build.yml # 多平台 CI + 发布
└── package.json                # Node.js 配置
```

---

## 前置要求

### 必需软件

| 工具 | 版本 | 说明 |
| --- | --- | --- |
| **Rust** | 1.77.2+ | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` |
| **Node.js** | 18+ | |
| **Tauri CLI** | — | `npm install -g @tauri-apps/cli` |
| **Go** | 1.22+ | 用于构建后端 —— https://go.dev/dl/ |

### 系统依赖

- **Windows** —— Microsoft Visual Studio C++ Build Tools、WebView2 Runtime。
- **macOS** —— Xcode Command Line Tools。
- **Linux** ——
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf
  ```
  无需 OpenSSL 开发包：外壳及其依赖的 Tauri 组件都不链接 TLS 后端，
  `openssl-sys`、`native-tls`、`rustls` 均不会进入构建。

---

## 开发

```bash
# 1. 进入项目目录
cd go-magic-desktop

# 2. 安装 JS 依赖
npm install

# 3. 克隆 Go Magic 主仓库（在同一父目录）
cd ../ && git clone https://github.com/magicwubiao/go-magic.git
cd go-magic-desktop

# 4. 将后端构建到 resources 目录
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# 5. 开发模式运行
npm run dev
```

### 测试、格式化与静态检查

```bash
npm test               # cargo test —— 纯函数单元测试
npm run test:web       # node --test —— 更新检查的版本比较逻辑
npm run fmt            # cargo fmt
npm run clippy         # cargo clippy --all-targets -- -D warnings
npm run version:sync   # 将当前 git tag 写入 package.json / Cargo.toml / tauri.conf.json
```

CI 在独立的 lint 任务中强制全部四项检查（`cargo fmt --check`、
`cargo clippy -D warnings`、`cargo test`、`npm run test:web`），均为
**阻断式**——lint 失败则多平台构建不再启动。

---

## 构建与打包

### 快速构建

```bash
npm run build          # 构建前端 + 打包桌面应用
npm run build:linux    # 或构建指定平台
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

CI 对四个目标执行同样的流程，只需推送 tag。

### 打包输出

构建完成后，安装包位于：

| 平台 | 位置 |
| --- | --- |
| Windows NSIS | `src-tauri/target/release/bundle/nsis/*.exe` |
| macOS | `src-tauri/target/release/bundle/dmg/*.dmg` |
| Linux AppImage | `src-tauri/target/release/bundle/appimage/*.AppImage` |
| Linux DEB | `src-tauri/target/release/bundle/deb/*.deb` |

---

## 如何工作

### 与 Go Magic 集成

Tauri 应用自动编排整个生命周期：

1. **检测** `resources` 目录中的 go-magic 可执行文件。
2. **启动** `go-magic server --port <PORT>`。
3. **健康检查** —— 等待就绪（60 秒预算，失败则弹出错误并退出）。
4. **加载** `http://127.0.0.1:<PORT>/` 到 WebView。
5. **显示**主窗口并聚焦。
6. 窗口关闭时**关闭**后端。

### 端口优先级

首选端口按 `5000 → 5001 → 5002` 依次尝试；都不可用时，在高位临时端口段
（`49152–65535`）内随机探测最多 64 个候选。随机化是为了避免两个实例（或
其他同样扫描固定端口段的程序）总是相中同一个候选——固定回退段（旧的
`8080`/`3000`）恰好也是开发服务器最爱占用的端口。

### 进程模型

```
┌─────────────────────────────────────┐
│         Tauri Main Process          │
│  ┌─────────────────────────────┐   │
│  │   Rust Backend Manager      │   │
│  │  · Process spawn / kill     │   │
│  │  · Health check             │   │
│  │  · Port allocation          │   │
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

### Tauri 命令

| 命令 | 用途 |
| --- | --- |
| `get_backend_port` | 获取后端运行端口 |
| `restart_backend_cmd` | 重启后端进程 |
| `get_app_info` | 获取版本、git 提交、构建时间与配置 |
| `check_backend_health_cmd` | 检查后端是否健康 |
| `notify_update_available` | 弹出原生"发现新版本"对话框并打开下载页 |
| `update_check_finished` | 关闭隐藏的更新检查窗口（由检查页自身调用） |

---

## 配置

### Tauri 配置

修改 `tauri.conf.json` 可调整窗口大小/标题、权限设置与打包选项。

### 环境变量

| 变量 | 说明 |
| --- | --- |
| `GOMAGIC_PORT` | 后端监听端口（同时以 `--port` 传入） |
| `RUST_BACKTRACE` | 仅在 **debug** 构建中为后端设为 `1` |

构建期元数据（`APP_VERSION`、`GIT_COMMIT`、`GIT_BRANCH`、`BUILD_TIME`、
`BUILD_PROFILE`）由 `src-tauri/build.rs` 注入，并通过 `get_app_info` 暴露。

---

## 更新检查

应用内置一套轻量的"检查 + 提示"更新流程——不引入就地更新器，也不增加依赖：

1. 每次启动时，外壳会打开一个**隐藏窗口**（`update-check`），加载
   `src-tauri/splash/update-check.html`。
2. 该页面**并发**拉取多个更新清单源——GitHub Releases API（权威、无缓存）
   加上 `master` 分支的 `latest.json`（经 jsDelivr / Raw，国内可达性好），
   取其中**最高**的版本，与当前运行版本（通过 `get_app_info`）比对。
3. 若存在更新版本，则弹出**原生对话框**，确认后在系统浏览器中打开清单里的
   `downloadUrl`；否则保持静默。
4. 用户关掉该弹窗（无论点哪个按钮）后，外壳会把 `(当前版本 -> 最新版本)`
   记录到应用数据目录的 `update-notice.json`，此后自动检查对同一对版本保持
   静默——不会每次启动都弹。菜单里的**检查更新...**不受影响，始终弹窗；
   应用升级或出现更新版本后记录不再匹配，提示会恢复。

之所以用隐藏窗口而不是 splash 页：后端热启动时端口几乎立即绑定，主窗口会在
第一秒内完成导航，splash 页里的 fetch 大概率被中途打断。隐藏窗口不延迟启动、
不受主窗口导航影响，每次启动都能完整跑完检查；另有 20 秒生命周期兜底，即使
网络挂起也会自动关闭。

### 菜单入口

主窗口同时带有原生的**关于(A)**下拉菜单：

- **关于 Go Magic** —— 原生对话框展示版本、git 提交与构建时间
- **检查更新...** —— 按需执行同样的检查；与启动时自动检查不同，它会始终
  反馈结果（"当前已是最新版本" / "检查失败" / 更新弹窗）
- **退出 Go Magic**（Alt+F4）—— 关停后端进程并退出应用

### 发布新版本

1. 打标签发布——CI 会构建安装包、创建 GitHub Release，并把 `latest.json`
   **自动发布到 `master`**（写入标签版本、以 Release 页为 `downloadUrl`、
   以 Release 正文为 `notes`），随后清理 jsDelivr 缓存。见
   `.github/workflows/build.yml` 的 `publish-manifest` 任务。已安装的客户端
   下次启动即可收到提示，无需人工干预。
2. 非标签构建不会改动清单，因此 master 上的坏构建不会把自己"报"成新版本。

### 自定义

- **清单来源** —— 修改 `src-tauri/splash/update-check.html` 中的
  `MANIFEST_SOURCES`。每项形如 `{ url, timeoutMs }`；页面会全部拉取并采信
  最高版本，因此 CDN 陈旧只会"少数"，不会漏掉已发布的新版本。任何返回下述
  JSON（或 GitHub `/releases/latest` 结构）的 URL 均可：GitHub Raw、自有
  服务器、OSS/CDN。
- **清单格式** —— `{"version": "1.2.3", "downloadUrl": "https://…", "notes": "…"}`
  版本按数字段逐位比较（容忍 `v` 前缀）；`downloadUrl` 必须是 http(s)，
  否则跳过提示。
- 整个检查是尽力而为：网络失败、清单缺失或请求挂起都不会阻塞启动、也不会
  报错打扰用户。

---

## 安全模型

- **CSP** —— 脚本/样式/连接目标仅限应用自身与本地后端，并附 `object-src 'none'`、
  `base-uri 'self'`、`frame-ancestors 'none'`。不允许 `'unsafe-eval'`；捆绑 UI 的
  内联脚本/样式仍需要 `'unsafe-inline'`，`img-src` 还额外允许 `https:` 与 `data:`。
  `connect-src` 额外允许 `https:`，供隐藏的更新检查窗口拉取更新清单——仅对本地
  页面生效，远程后端 UI 不受该 CSP 约束。
- **端口绑定** —— 后端只绑定 `127.0.0.1`。
- **权限** —— `src-tauri/capabilities/default.json` 仅授予 `core:default`，且仅对
  **本地**内容生效。Go Magic UI 由捆绑后端通过 `http://127.0.0.1:<port>` 提供，
  Tauri 会将其视为*远程*内容——因此该 UI 刻意**没有**任何 Tauri 命令或插件权限。
  若该来源确实需要 IPC，请显式添加 `remote` capability，而不是放宽默认 capability。
- **release 不含 devtools** —— `devtools` cargo feature 默认关闭，且窗口构建时在
  非 `debug_assertions` 下显式禁用 devtools。
- **无 TLS** —— 外壳自身不引入 HTTP 客户端；其唯一请求（对 `127.0.0.1` 的
  `GET /health`）走裸 socket。`Cargo.lock` 中不含任何 TLS crate（`openssl`、
  `native-tls`、`rustls`、`ring` 全部缺失），桌面构建也不会引入 `reqwest`/`hyper`
  —— Tauri 仅在非桌面目标上需要它们。
- **外部链接** —— 仅将 `http`/`https` 交给系统处理。

---

## 日志与排查

### 日志位置

- **Windows** —— `%APPDATA%/go-magic-desktop/logs/`
- **macOS** —— `~/Library/Logs/go-magic-desktop/`
- **Linux** —— `~/.local/share/go-magic-desktop/logs/`

### 日志文件

- `go-magic.log` —— 应用主日志
- `go-magic.log.0` —— 轮转日志

### 常见问题

**WebView2 缺失（Windows）**
```powershell
winget install Microsoft.WebView2
```

**后端启动失败**
1. 检查 `src-tauri/resources/go-magic.exe` 是否存在。
2. 查看日志文件中的错误信息。
3. 验证端口是否被占用。

**macOS 安全提示**
系统偏好设置 → 安全性与隐私 → 允许运行。

---

## CI/CD 与发布

自动化构建基于 GitHub Actions：

- **lint** —— 全量 `cargo fmt --check`、`cargo clippy -D warnings`、
  `cargo test` 与更新检查版本比较的 node 测试（全部阻断式）。
- **build** —— 多平台构建（Windows / macOS-x64 / macOS-arm64 / Linux），启用 npm
  与 cargo 缓存，以 lint 任务为前置门禁，并在某平台没有产出安装包时直接失败。
- **release** —— 标签推送时自动发布 GitHub Release 并附带校验和。

后端从 `magicwubiao/go-magic` 的 `GO_MAGIC_REF`（workflow 输入或仓库变量，默认
`main`）构建——建议固定为 tag 或 commit SHA 以保证可复现。

当存在 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、
`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` secrets
时，macOS 构建会自动签名与公证；不存在时会静默跳过。

---

## 版本管理

版本号**完全由 git 标签派生**——不手动维护任何配置文件中的版本字段。
`package.json`、`Cargo.toml` 和 `tauri.conf.json` 均保持占位符 `0.0.0`。

构建时，`build.rs` 决定 `APP_VERSION`：CI 标签推送会先执行
`scripts/sync-version.mjs <tag>` 把标签版本写入 `package.json`、
`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 与 `latest.json`（更新检查
清单），`build.rs` 优先采用 `Cargo.toml` 里已写入的真实版本；本地构建
（`Cargo.toml` 仍是 `0.0.0`）则回退到 `git describe --tags`。**优先打过的
版本号而不是 describe**，是因为同一提交上存在两个标签时 describe 会解析到
较旧的那个（v0.6.23 与 v0.6.24 曾指向同一提交，导致"0.6.24"安装包自报
0.6.23、无限提示更新）。

### 发布流程

```bash
git tag v1.0.0
git push origin v1.0.0
```

CI 将自动构建 Windows、macOS（x64 + arm64）、Linux 版本，并创建附带校验和的
GitHub Release。

### 本地构建

没有标签的本地构建会生成版本 `0.0.0-dev`。要使用特定版本构建，先创建标签：

```bash
git tag v0.5.1
npm run build
```

---

## 许可证与贡献

- **许可证**：MIT
- **贡献**：欢迎提交 Issue 和 Pull Request！