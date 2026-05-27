# Go Magic Desktop (Tauri)

[English](README.md) | [中文](README.zh-CN.md)

Tauri desktop application that packages Go Magic as a cross-platform desktop app.

📖 **Installation Guide** → See [BUILD_GUIDE.md](BUILD_GUIDE.md) for detailed installation instructions.

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
- **Logging System**: Structured logging for troubleshooting
- **Security Policy**: CSP protection, permission control

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

4. **Go** (1.21+) - For building backend
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
4. **UI Loading**: WebView loads `http://[IP]:<PORT>/`
5. **Window Management**: Show main window and focus
6. **Graceful Shutdown**: Terminate backend process on window close

### Port Priority

Auto-select available port: 5000 → 5001 → 5002 → 5003 → 5004 → 8080 → 3000

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

## License

MIT License

## Contributing

Issues and Pull Requests are welcome!
