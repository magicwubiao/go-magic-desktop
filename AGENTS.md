# Go Magic Desktop - Agent 规范文档

## 项目概述

**项目名称**: Go Magic Desktop
**项目类型**: Tauri 桌面应用
**核心功能**: 将 Go Magic AI 开发助手打包为跨平台桌面应用
**技术栈**: Rust (Tauri 2.0) + TypeScript + Node.js

## 技术架构

### 组件架构

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                     │
│  ┌─────────────────────────────────────────────────────┐│
│  │                   Rust Runtime                       ││
│  │  ┌──────────────┐  ┌──────────────┐  ┌───────────┐││
│  │  │ Process Mgr  │  │  Window Mgr  │  │ Log Mgr   │││
│  │  │ (go-magic)   │  │  (WebView2)  │  │ (tauri-   │││
│  │  │              │  │              │  │  log)     │││
│  │  └──────┬───────┘  └──────┬───────┘  └───────────┘││
│  │         │                  │                        ││
│  │         │    ┌──────────────┘                        ││
│  │         │    │                                      ││
│  │         ▼    ▼                                      ││
│  │  ┌─────────────────────────────────────────────┐    ││
│  │  │          Go Magic Backend Service           │    ││
│  │  │              (localhost:PORT)               │    ││
│  │  └─────────────────────────────────────────────┘    ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
```

### 进程管理策略

1. **自动端口选择**: 按优先级尝试 [5000, 5001, 5002, 5003, 5004, 8080, 3000]
2. **健康检查**: 60秒超时，每500ms重试，支持 `/health` 和根路径
3. **进程隔离**: 后端运行在独立进程中，Windows 下隐藏控制台窗口
4. **优雅关闭**: 窗口关闭时自动终止后端进程

### 端口管理

- 默认端口: 5000
- 备用端口: 5001-5004, 8080, 3000
- 绑定地址: 127.0.0.1 (仅本地访问)
- 端口选择: 自动检测可用端口，避免冲突

## 目录结构

```
go-magic-desktop/
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   └── main.rs              # 主程序入口
│   ├── Cargo.toml               # Rust 依赖
│   ├── build.rs                 # 构建脚本
│   ├── tauri.conf.json         # Tauri 配置
│   ├── tauri.dev.conf.json     # 开发配置
│   ├── resources/               # 打包资源 (go-magic 二进制)
│   └── icons/                   # 应用图标
├── icons/                        # 图标源文件
├── web-dist/                     # 前端构建输出
├── .github/workflows/           # CI/CD 配置
├── build-all.sh                # 多平台构建脚本
├── build-windows.ps1           # Windows 构建脚本
├── package.json                # Node.js 配置
├── README.md                   # 项目文档
└── AGENTS.md                   # 本文档

go-magic/                        # Go Magic 主仓库 (同级目录)
├── web/                        # 前端源码
├── internal/server/dist/       # 前端构建输出
└── cmd/magic/                  # Go 后端入口
```

## 运行与预览

### 开发模式

```bash
# 安装依赖
npm install

# 启动开发服务器
npm run dev
```

### 构建命令

```bash
# 构建前端 + 打包桌面应用
npm run build

# 分步执行
npm run build:web      # 构建前端
tauri build            # 打包桌面应用

# 多平台构建
./build-all.sh all     # 所有平台
./build-all.sh windows # 仅 Windows
./build-all.sh macos  # macOS Intel
./build-all.sh linux   # Linux x64
```

### 依赖要求

- **Rust**: 1.75+
- **Node.js**: 18+
- **Tauri CLI**: 2.0+
- **Go**: 1.21+ (用于构建后端)

### 系统依赖

- **Windows**: WebView2 Runtime
- **macOS**: 无额外依赖
- **Linux**: libwebkit2gtk-4.1-dev, libssl3

## 关键配置

### tauri.conf.json 关键字段

| 字段 | 说明 |
|------|------|
| `productName` | 应用显示名称 |
| `identifier` | 应用唯一标识 |
| `app.windows` | 窗口配置 |
| `bundle.resources` | 打包资源 (go-magic 二进制) |
| `app.security.csp` | 内容安全策略 |
| `plugins.*` | 插件权限配置 |

### 后端启动参数

```rust
// main.rs 中的启动命令
Command::new(backend_path)
    .args(["server", "--port", &port.to_string()])
    .env("GOMAGIC_PORT", port.to_string())
    .env("RUST_BACKTRACE", "1")
```

## 构建产物

### 输出位置

| 平台 | 安装包位置 |
|------|------------|
| Windows | `src-tauri/target/release/bundle/nsis/*.exe` |
| Windows MSI | `src-tauri/target/release/bundle/msi/*.msi` |
| macOS | `src-tauri/target/release/bundle/dmg/*.dmg` |
| Linux AppImage | `src-tauri/target/release/bundle/appimage/*.AppImage` |
| Linux DEB | `src-tauri/target/release/bundle/deb/*.deb` |

### 资源打包

后端二进制文件通过 `bundle.resources` 配置打包:
```json
{
  "resources": {
    "go-magic*": "./resources/*"
  }
}
```

## 安全策略

### CSP 配置

```json
"csp": "default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; connect-src 'self' http://127.0.0.1:* http://localhost:*"
```

### 权限控制

- **Shell**: 仅允许打开外部链接
- **FS**: 限制对敏感目录的访问
- **HTTP**: 仅允许本地和 HTTPS 连接

## 日志管理

### 日志位置

- **Windows**: `%APPDATA%/go-magic-desktop/logs/`
- **macOS**: `~/Library/Logs/go-magic-desktop/`
- **Linux**: `~/.local/share/go-magic-desktop/logs/`

### 日志级别

- 开发模式: DEBUG
- 生产模式: INFO

### 日志内容

- 应用启动/关闭
- 后端进程状态
- 健康检查结果
- 错误堆栈

## 常见问题和预防

### 1. WebView2 缺失 (Windows)

**症状**: 程序启动后立即退出
**解决**: 安装 WebView2 Runtime
```powershell
winget install Microsoft.WebView2
```

### 2. 后端启动失败

**症状**: 窗口显示但无内容
**排查**: 检查日志中的后端启动错误
**解决**:
1. 确保 `resources/go-magic.exe` 存在
2. 检查端口是否被占用
3. 验证 go-magic 版本兼容性

### 3. macOS 安全提示

**症状**: "无法打开，因为来自身份不明的开发者"
**解决**: 在系统偏好设置中允许运行

### 4. Linux 缺少依赖

**症状**: 启动时报 `libwebkit2gtk` 错误
**解决**:
```bash
sudo apt install libwebkit2gtk-4.1-dev libssl3
```

## CI/CD 配置

### GitHub Actions 工作流

- **lint**: 代码检查 (fmt, clippy, test)
- **build-desktop**: 多平台桌面应用构建
- **build-go-cli**: Go CLI 多平台构建
- **create-release**: 自动发布 (仅 tag 触发)

### 构建缓存

- Cargo registry 和 git 缓存
- Cargo target 目录缓存
- Node.js 模块缓存

## 用户偏好与长期约束

1. **端口管理**: 不硬编码端口，必须支持多实例
2. **日志**: 必须记录后端进程状态，便于排查
3. **错误恢复**: 后端异常退出应有明确提示
4. **兼容性**: 保持向后兼容，支持旧版本配置

## 优化记录

### 架构选择：进程分离模式

经过分析，选择进程分离模式作为最佳方案：

| 因素 | 分析结论 |
|------|----------|
| go-magic 形态 | 独立 Go 程序，非库 |
| 项目关系 | 桌面版依赖 go-magic 主项目 |
| 版本管理 | CLI 和桌面版共用同一后端 |
| 维护成本 | 进程分离更易维护同步 |

**优势：**
- 无需修改 go-magic 源码
- CLI 和桌面版完全共享后端
- 版本升级只需替换二进制
- 构建流程简单

### 2024-01 版本

- 添加自动端口选择机制
- 实现健康检查超时控制
- 添加 Tauri 日志插件集成
- 完善 Windows 进程管理 (隐藏控制台)
- 优化 CI/CD 构建流程
- 添加代码质量检查
- 完善安全策略 (CSP, 权限)
