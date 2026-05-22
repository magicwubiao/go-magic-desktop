# Go Magic Desktop (Tauri)

Tauri 桌面端应用，将 Go Magic 打包为跨平台桌面应用。

## 项目结构

```
go-magic-tauri/
├── src-tauri/          # Rust 后端代码
│   ├── src/
│   │   └── main.rs     # 主程序入口
│   ├── Cargo.toml      # Rust 依赖
│   └── build.rs        # 构建脚本
├── icons/              # 应用图标
├── tauri.conf.json     # Tauri 配置
├── package.json        # Node.js 配置
└── README.md           # 本文档
```

## 前置要求

1. **Rust** (1.70+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Node.js** (18+)

3. **Tauri CLI**
   ```bash
   npm install -g @tauri-apps/cli
   ```

4. **系统依赖**
   - **Windows**: Microsoft Visual Studio C++ Build Tools
   - **macOS**: Xcode Command Line Tools
   - **Linux**: `sudo apt install libwebkit2gtk-4.0-dev libssl-dev`

## 开发

```bash
# 进入项目目录
cd go-magic-tauri

# 安装依赖
npm install

# 开发模式（热重载）
npm run dev
```

## 构建

```bash
# 构建前端并打包桌面应用
npm run build

# 或分步执行
npm run build:web    # 构建前端
tauri build          # 打包桌面应用
```

## 打包输出

构建完成后，安装包位于：
- **Windows**: `src-tauri/target/release/bundle/msi/*.msi`
- **macOS**: `src-tauri/target/release/bundle/dmg/*.dmg`
- **Linux**: `src-tauri/target/release/bundle/deb/*.deb`

## 与 Go Magic 集成

Tauri 应用会自动：
1. 启动 Go 后端服务 (`go-magic server`)
2. 加载前端界面 (`../go-magic/web/dist`)
3. 应用关闭时停止后端服务

## 配置

修改 `tauri.conf.json` 可调整：
- 窗口大小和标题
- 权限设置
- 打包选项

## 注意事项

1. 首次构建会下载 Rust 依赖，可能需要几分钟
2. 确保 `../go-magic/web/dist` 存在（运行 `npm run build:web` 生成）
3. 生产环境需要将 `go-magic` 二进制文件放在应用目录

## 快速构建

```bash
# 一键构建所有平台
./build-all.sh

# 或分步执行
npm install                    # 安装依赖
npm run build:web             # 构建前端
tauri build                   # 构建桌面应用
```

## 多平台构建

### Windows
```bash
tauri build --target x86_64-pc-windows-msvc
```

### macOS
```bash
tauri build --target x86_64-apple-darwin   # Intel
tauri build --target aarch64-apple-darwin  # Apple Silicon
```

### Linux
```bash
tauri build --target x86_64-unknown-linux-gnu    # x64
tauri build --target aarch64-unknown-linux-gnu   # ARM64
```

## 输出文件

构建完成后，安装包位于：
- **Windows**: `src-tauri/target/release/bundle/msi/`
- **macOS**: `src-tauri/target/release/bundle/dmg/`
- **Linux**: `src-tauri/target/release/bundle/deb/` 或 `appimage/`
