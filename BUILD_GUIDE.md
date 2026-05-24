# Go Magic Desktop 构建指南

## 简单方法：使用 GitHub Actions（推荐）

这是最简单的方式，不需要在本地配置复杂的开发环境。

### 步骤：

1. **Fork 这个仓库** 到你自己的 GitHub 账号

2. **触发构建**：
   - 点击仓库顶部的 "Actions" 标签
   - 选择 "Build Desktop App" workflow
   - 点击 "Run workflow" → 选择 main 分支 → 点击绿色的 "Run workflow" 按钮

3. **下载构建产物**：
   - 等待 workflow 运行完成（大约需要 10-20 分钟）
   - 点击完成的 workflow 运行记录
   - 在页面底部的 "Artifacts" 区域下载对应你系统的安装包：
     - Windows: `go-magic-desktop-windows-x64`
     - macOS (Intel): `go-magic-desktop-macos-x64`
     - macOS (Apple Silicon): `go-magic-desktop-macos-arm64`
     - Linux: `go-magic-desktop-linux-x64`

4. **安装和使用**：
   - **Windows**: 下载后解压，运行 `.exe` 或 `.msi` 安装程序，一路下一步即可。安装完成后双击桌面上的 "Go Magic" 图标就能用了。
   - **macOS**: 下载后打开 `.dmg` 文件，把 "Go Magic" 拖到 Applications 文件夹。然后在 Launchpad 或应用程序文件夹中找到并双击打开。
   - **Linux**: 下载后解压，运行 `.AppImage` 文件（需要先给执行权限：`chmod +x *.AppImage`）或安装 `.deb` 包。

---

## 本地构建方法（高级用户）

如果你想在自己的电脑上构建，需要以下步骤：

### 前置条件

1. **安装 Rust**: https://rustup.rs/
2. **安装 Node.js**: https://nodejs.org/ (推荐 v18 或更高)
3. **安装 Go**: https://golang.org/dl/ (推荐 1.21 或更高)
4. **系统依赖**：
   - **Windows**: Visual Studio C++ Build Tools
   - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
   - **Linux**: 参照 Tauri 文档安装系统依赖

### 构建步骤

1. **克隆代码**：
```bash
git clone https://github.com/magicwubiao/go-magic-desktop.git
cd go-magic-desktop
```

2. **克隆 go-magic 后端仓库**（构建时需要）：
```bash
git clone https://github.com/magicwubiao/go-magic.git
```

3. **安装前端依赖**：
```bash
npm install
```

4. **构建前端**（在 go-magic 目录中）：
```bash
cd go-magic/web
npm install
npm run build
cd ../..
```

5. **构建后端**：
```bash
cd go-magic
CGO_ENABLED=0 go build -ldflags="-s -w" -o ../src-tauri/resources/go-magic ./cmd/magic
cd ..
```

6. **确保后端有执行权限**（Linux/macOS）：
```bash
chmod +x src-tauri/resources/go-magic
```

7. **构建桌面应用**：
```bash
cd src-tauri
npm install
npm run tauri build
```

8. **找到安装包**：
   - Windows: `src-tauri/target/release/bundle/nsis/`
   - macOS: `src-tauri/target/release/bundle/dmg/`
   - Linux: `src-tauri/target/release/bundle/appimage/` 或 `deb/`

---

## 常见问题

### 应用打开后没反应？

1. 确保你的电脑有网络连接（有些功能可能需要）
2. 查看是否有错误弹窗提示
3. Windows 上可以尝试右键 → 以管理员身份运行
4. 如果持续有问题，请提 Issue 并附上系统信息

### macOS 提示"无法打开，因为无法验证开发者"？

打开 "系统设置" → "隐私与安全性" → 向下滚动找到 "安全性" 部分，点击 "仍要打开"。

### 我的构建失败了？

推荐使用 GitHub Actions 构建，它已经配置好了所有环境，更稳定。
