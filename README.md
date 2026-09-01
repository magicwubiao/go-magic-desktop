# Go Magic Desktop (Tauri)

[English](README.md) | [中文](README.zh-CN.md)

Tauri desktop application that packages Go Magic as a cross-platform desktop app.

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                        │
│                                                            │
│  ┌────────────────┐     ┌─────────────────────────────┐   │
│  │   WebView      │     │    go-magic Backend         │   │
│  │   (UI)         │◀───▶│    - HTTP Server (port)    │   │
│  │                │     │    - API Handler           │   │
│  │   localhost    │     │    - Standalone Process    │   │
│  └────────────────┘     └─────────────────────────────┘   │
│                                                            │
│  Features:                                                 │
│  ✓ Shared backend between CLI and desktop                  │
│  ✓ No modifications to go-magic source code                │
│  ✓ Easy version upgrades by replacing binary               │
│  ✓ Window state persistence (position & size)              │
│  ✓ High-DPI aware window positioning                      │
│  ✓ External links open in system browser                   │
└────────────────────────────────────────────────────────────┘
```

**Why Process Separation?**
- go-magic is a standalone Go program, not a library
- CLI and desktop versions share the same backend completely
- Easy maintenance and version synchronization

## Features

- **Cross-Platform**: Windows, macOS, Linux
- **Embedded Backend**: Go Magic backend auto-starts and manages
- **Auto Port Selection**: Smart port selection to avoid conflicts
- **Health Check**: 60-second timeout for backend readiness detection
- **Window State Persistence**: Remembers window position and size across sessions
- **High-DPI Support**: Correct window positioning and sizing under display scaling
- **External Link Handling**: Opens non-local links in the system browser automatically
- **Backend Restart**: Restart backend from the UI without closing the app
- **App Info Query**: Retrieve version, git commit, and build metadata at runtime
- **Logging System**: Structured logging for troubleshooting
- **Security Policy**: CSP protection, permission control

## Usage

### Quick Start

1. **Download & Install** — Get the installer for your platform from the [Releases](https://github.com/magicwubiao/go-magic-desktop/releases) page:
   - Windows: `.exe` (NSIS) or `.msi`
   - macOS: `.dmg` (Intel & Apple Silicon)
   - Linux: `.AppImage` or `.deb`

2. **Launch** — Open the app (the window is titled **"Go Magic"**). The desktop app automatically:
   - locates the bundled `go-magic` backend executable,
   - picks an available port (default `5000`, falling back to `5001` / `5002` / … / `3000`),
   - starts the backend and waits up to 60 seconds for it to pass a health check,
   - loads the Go Magic web UI inside the window.

3. **Use the app** — Everything runs inside the window. This is the standard Go Magic web interface; no extra setup is required.

### Backend Management

- The backend process is **managed automatically**. It starts with the app and is terminated when you close the window.
- **Restart from the UI**: if the backend becomes unresponsive, trigger the in-app restart action (calls `restart_backend_cmd`) — no need to quit the app.
- **App info**: version, git commit, and build metadata are queryable at runtime via `get_app_info`.

### Links & Navigation

- Links to `127.0.0.1` / `localhost` open inside the app.
- Any **external link** is automatically opened in your **system default browser**.

### Window & Layout

- Default window size is 1024×800 (minimum 800×600).
- Window position and size are **persisted across sessions**; the app restores them on the next launch and adapts to Hi-DPI displays.

### Exiting

- Closing the window quits the app. The app saves the window state and gracefully stops the backend process.

### Logs & Troubleshooting

Log locations:
- **Windows**: `%APPDATA%/go-magic-desktop/logs/`
- **macOS**: `~/Library/Logs/go-magic-desktop/`
- **Linux**: `~/.local/share/go-magic-desktop/logs/`

If the app fails to start, see [FAQ](#faq) below.

## Project Structure

```
go-magic-desktop/
├── src-tauri/          # Rust backend code
│   ├── src/
│   │   └── main.rs     # Main entry point
│   ├── Cargo.toml      # Rust dependencies
│   ├── tauri.conf.json # Tauri configuration
│   └── resources/      # Packaging resources
├── icons/              # Application icons
├── build-all.sh        # Multi-platform build script
├── build-windows.ps1   # Windows build script
└── package.json        # Node.js configuration
```

## Prerequisites

### Required Software

1. **Rust** (1.75+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Node.js** (18+)

3. **Tauri CLI**
   ```bash
   npm install -g @tauri-apps/cli
   ```

4. **Go** (1.26+) - For building backend
   ```bash
   go install golang.org/dl/go1.21@latest
   ```

### System Dependencies

- **Windows**: Microsoft Visual Studio C++ Build Tools, WebView2 Runtime
- **macOS**: Xcode Command Line Tools
- **Linux**:
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libssl3 libgtk-3-dev
  ```

## Development

```bash
# Enter project directory
cd go-magic-desktop

# Install dependencies
npm install

# Clone Go Magic main repository (in the same parent directory)
cd ../ && git clone https://github.com/magicwubiao/go-magic.git

# Return to desktop app directory
cd go-magic-desktop

# Build Go Magic backend
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# Development mode
npm run dev
```

## Building

### Quick Build

```bash
# Build frontend and package desktop app
npm run build

# Or use build script
./build-all.sh
```

### Step-by-Step Build

```bash
# 1. Build frontend
cd ../go-magic/web && npm run build

# 2. Build backend
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# 3. Package desktop app
cd ../go-magic-desktop && tauri build
```

### Multi-Platform Build

```bash
# All platforms
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

## Build Output

After building, installers are located at:

| Platform | Location |
|----------|----------|
| Windows NSIS | `src-tauri/target/release/bundle/nsis/*.exe` |
| Windows MSI | `src-tauri/target/release/bundle/msi/*.msi` |
| macOS | `src-tauri/target/release/bundle/dmg/*.dmg` |
| Linux AppImage | `src-tauri/target/release/bundle/appimage/*.AppImage` |
| Linux DEB | `src-tauri/target/release/bundle/deb/*.deb` |

## Go Magic Integration

The Tauri app automatically handles:

1. **Backend Detection**: Find go-magic executable in resources directory
2. **Backend Startup**: Auto-start `go-magic server --port <PORT>`
3. **Health Check**: Wait for backend readiness (60s timeout)
4. **UI Loading**: WebView loads `http://127.0.0.1:<PORT>/`
5. **Window Management**: Show main window and focus
6. **Graceful Shutdown**: Terminate backend process on window close

### Port Priority

Auto-select available port: 5000 → 5001 → 5002 → 5003 → 5004 → 8080 → 3000

### Tauri Commands

| Command | Description |
|---------|-------------|
| `get_backend_port` | Get the port the backend is running on |
| `restart_backend_cmd` | Restart the backend process |
| `get_app_info` | Get app version, git commit, build time, and profile |
| `check_backend_health_cmd` | Check if the backend is healthy |

## Configuration

### Tauri Configuration

Modify `tauri.conf.json` to adjust:
- Window size and title
- Permission settings
- Packaging options

### Environment Variables

| Variable | Description |
|----------|-------------|
| `GOMAGIC_PORT` | Backend listening port |
| `RUST_BACKTRACE` | Rust stack trace level |

## Architecture

### Process Management

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
│  │   (Standalone Subprocess)   │   │
│  └─────────────┬───────────────┘   │
│                │                    │
│  ┌─────────────▼───────────────┐   │
│  │   WebView (User Interface)  │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
```

### Security Policy

- **CSP**: Restrict script sources and connection targets
- **Port Binding**: Only allow localhost access
- **Permission Control**: Only allow necessary system operations

## Logs

Log locations:
- **Windows**: `%APPDATA%/go-magic-desktop/logs/`
- **macOS**: `~/Library/Logs/go-magic-desktop/`
- **Linux**: `~/.local/share/go-magic-desktop/logs/`

Log files:
- `go-magic.log`: Application main log
- `go-magic.log.0`: Rotated log

## FAQ

### WebView2 Missing (Windows)

```powershell
winget install Microsoft.WebView2
```

### Backend Startup Failure

1. Check if `src-tauri/resources/go-magic.exe` exists
2. Review error messages in log files
3. Verify if port is occupied

### macOS Security Warning

System Preferences → Security & Privacy → Allow anyway

## CI/CD

Using GitHub Actions for automated builds:

- **build**: Multi-platform desktop app build (Windows/macOS-x64/macOS-arm64/Linux)
- **release**: Automated GitHub Release with checksums on tag push

## Versioning

Versions are derived **entirely from git tags** — no version field in any config
file is maintained manually. `package.json`, `Cargo.toml`, and `tauri.conf.json`
all keep a placeholder `0.0.0`.

At build time, `build.rs` runs `git describe --tags` to embed the real version
(plus git commit, build time, build profile) into the binary. On CI tag pushes,
the tag version is also injected into `tauri.conf.json` so the installer package
carries the correct version.

### Release Flow

```bash
# Just tag and push — that's it
git tag v1.0.0
git push origin v1.0.0
```

CI then automatically builds for Windows, macOS (x64 + arm64), Linux, and
creates a GitHub Release with checksums.

### Local Builds

Local builds without a tag produce version `0.0.0-dev`. To build with a
specific version locally, create a tag first:

```bash
git tag v0.5.1
npm run build
```

## License

MIT License

## Contributing

Issues and Pull Requests are welcome!