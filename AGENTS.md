## 项目概述

Go Magic Desktop —— 基于 Tauri 2.x 的跨平台桌面应用，将 Go Magic（AI 开发助手）以桌面应用形式交付。采用进程隔离架构：Tauri Rust 后端负责窗口管理和生命周期，内嵌的 Go 后端（go-magic）提供 HTTP API 服务，WebView 加载前端页面。

## 技术栈

- **桌面框架**：Tauri 2.1（Rust 后端 + WebView 前端）
- **前端**：静态资源部署在 `web-dist/`，开发时通过 `devUrl: http://localhost:5000` 加载
- **内嵌后端**：Go Magic（独立 Go 进程，由 Tauri 启动和管理）
- **Rust 依赖**：tauri 2.1、tauri-plugin-{fs,os,process,log,dialog,http}、reqwest、serde、chrono
- **Node.js 依赖**：@tauri-apps/cli 2.1、@tauri-apps/api 2.1 及相关插件
- **包管理**：npm（项目原有，有 package-lock.json）
- **构建工具**：Tauri CLI（`tauri dev` / `tauri build`）
- **运行时要求**：Node.js 18+、Rust 1.75+、Go 1.21+

## 目录结构

```
go-magic-desktop/
├── src-tauri/              # Rust 后端
│   ├── src/main.rs         # 主入口（窗口管理、Go 进程启动、健康检查）
│   ├── Cargo.toml          # Rust 依赖
│   ├── tauri.conf.json     # Tauri 生产配置
│   ├── tauri.dev.conf.json # Tauri 开发配置
│   ├── build.rs            # 构建脚本
│   ├── icons/              # 应用图标
│   └── resources/          # 打包资源（Go 二进制等）
├── package.json            # Node.js 配置
├── package-source.sh       # 打包脚本
├── .github/                # CI/CD 配置
└── README.md / README.zh-CN.md
```

## 关键入口 / 核心模块

- `src-tauri/src/main.rs`：应用主入口，包含窗口状态管理、Go 后端进程启动与健康检查（60s 超时）、自动端口选择、日志系统
- `src-tauri/tauri.conf.json`：Tauri 配置，CSP 安全策略、打包目标、图标配置
- `package.json`：开发/构建脚本入口（`tauri dev`、`tauri build`）

## 运行与预览

- **类型**：桌面应用（Tauri），当前平台不支持预览，`preview_enable = "disabled"`
- **开发**：`npm run dev`（需要 Rust + Go 环境）
- **构建**：`npm run build`（或平台特定 `build:windows` / `build:macos` / `build:linux`）
- **架构**：WebView 加载前端 → HTTP 调用内嵌 Go 后端 API

## 用户偏好与长期约束

- 项目原有使用 npm 包管理（package-lock.json），暂保留

## 常见问题和预防

- Go 后端二进制需要预先编译并放入 `src-tauri/resources/` 目录
- 开发时需要同时具备 Rust、Node.js、Go 三个运行时
- Linux 构建需要系统安装 `libwebkit2gtk-4.1-dev libssl3 libgtk-3-dev`
