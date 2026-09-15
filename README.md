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
- **Instant Window**: the window appears immediately — the backend health check runs on a background thread instead of blocking startup for up to 60s
- **Health Check**: 60-second budget for backend readiness detection, with throttled `backend-status` progress events
- **Reliable Process Management**: backend stdout/stderr is drained continuously (no pipe deadlock), the child is reaped on exit, and the whole process tree is killed on Windows
- **Window State Persistence**: Remembers window position and size across sessions
- **High-DPI Support**: Correct window positioning and sizing under display scaling
- **External Link Handling**: Opens non-local links in the system browser automatically (http/https only)
- **Backend Restart**: Restart the backend from the UI without closing the app and without blocking the UI thread
- **App Info Query**: Retrieve version, git commit, git branch, and build metadata at runtime
- **Logging System**: Structured logging to stdout **and** a log file, covering both the shell and the backend
- **Security Hardened**: least-privilege capabilities, strict CSP, no TLS stack linked into the binary, no webview devtools in release builds
- **Tested**: unit tests for the pure helpers, `cargo fmt --check` enforced in CI, clippy still advisory

## Usage

### Quick Start

1. **Download & Install** — Get the installer for your platform from the [Releases](https://github.com/magicwubiao/go-magic-desktop/releases) page:
   - Windows: `.exe` (NSIS) or `.msi`
   - macOS: `.dmg` (Intel & Apple Silicon)
   - Linux: `.AppImage` or `.deb`

2. **Launch** — Open the app (the window is titled **"Go Magic"**). The desktop app automatically:
   - locates the bundled `go-magic` backend executable,
   - picks an available port (default `5000`, falling back to `5001` / `5002` / … / `3000`),
   - shows the window right away while it starts the backend and waits up to 60 seconds for it to pass a health check,
   - loads the Go Magic web UI inside the window as soon as the backend answers.

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
├── src-tauri/                  # Rust shell + backend supervision
│   ├── src/main.rs             # Main entry point (process management, window)
│   ├── build.rs                # Embeds version/commit/build-time metadata
│   ├── Cargo.toml              # Rust dependencies
│   ├── tauri.conf.json         # Tauri configuration (window, CSP, bundling)
│   ├── capabilities/           # Least-privilege permission sets
│   ├── icons/                  # Application icons
│   └── resources/              # Bundled backend binary (populated at build time)
├── scripts/sync-version.mjs    # Stamps the git tag into all version fields
├── .github/workflows/build.yml # Multi-platform CI + release
└── package.json                # Node.js configuration
```

## Prerequisites

### Required Software

1. **Rust** (1.77.2+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Node.js** (18+)

3. **Tauri CLI**
   ```bash
   npm install -g @tauri-apps/cli
   ```

4. **Go** (1.22+) — for building the backend
   ```bash
   # https://go.dev/dl/
   ```

### System Dependencies

- **Windows**: Microsoft Visual Studio C++ Build Tools, WebView2 Runtime
- **macOS**: Xcode Command Line Tools
- **Linux**:
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf
  ```
  No OpenSSL development package is needed: neither the shell nor the Tauri crates
  it depends on link a TLS backend, so `openssl-sys`, `native-tls` and `rustls`
  never enter the build.

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

### Tests, formatting and linting

```bash
npm test               # cargo test — unit tests for the pure helpers
npm run fmt            # cargo fmt
npm run clippy         # cargo clippy --all-targets -- -D warnings
npm run version:sync   # stamp the current git tag into package.json / Cargo.toml / tauri.conf.json
```

CI enforces `cargo fmt --check` in a dedicated lint job; `cargo clippy` runs in
advisory mode — drop `continue-on-error` in `.github/workflows/build.yml` once the
tree is clippy-clean.

## Building

### Quick Build

```bash
# Build frontend and package desktop app
npm run build

# Or build for a specific platform
npm run build:linux
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

Cross-compiling a Tauri app is impractical, so each platform is built on its own
runner (or machine) through the npm scripts:

```bash
npm run build:windows    # x86_64-pc-windows-msvc
npm run build:macos      # x86_64-apple-darwin
npm run build:macos-arm  # aarch64-apple-darwin
npm run build:linux      # x86_64-unknown-linux-gnu
```

CI does the same for all four targets — just push a tag.

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
3. **Health Check**: Wait for backend readiness (60s budget, then show an error and exit)
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
| `GOMAGIC_PORT` | Backend listening port (also passed as `--port`) |
| `RUST_BACKTRACE` | Set to `1` for the backend in **debug** builds only |

Bundle-time metadata (`APP_VERSION`, `GIT_COMMIT`, `GIT_BRANCH`, `BUILD_TIME`,
`BUILD_PROFILE`) is injected by `src-tauri/build.rs` and surfaced through
`get_app_info`.

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

- **CSP**: script/style/connect targets are restricted to the app itself and the
  local backend, plus `object-src 'none'`, `base-uri 'self'` and
  `frame-ancestors 'none'`. `'unsafe-eval'` is not allowed; `'unsafe-inline'` is
  still required by the bundled UI's inline script/style, and `img-src`
  additionally allows `https:` and `data:` images.
- **Port Binding**: the backend only ever binds `127.0.0.1`.
- **Permissions**: `src-tauri/capabilities/default.json` grants only
  `core:default`, and only to local content. The Go Magic UI is served by the
  bundled backend over `http://127.0.0.1:<port>`, which Tauri classifies as
  *remote* content — so the UI deliberately gets **no** access to Tauri commands
  or plugins. If the UI ever needs IPC from that origin, add an explicit `remote`
  capability rather than widening the default one.
- **No devtools in release**: the `devtools` cargo feature is off by default and
  the window builder disables devtools when `debug_assertions` is off.
- **TLS**: the shell brings no HTTP client of its own — its only request
  (`GET /health` on `127.0.0.1`) goes over a raw socket. `Cargo.lock` carries no
  TLS crate at all (`openssl`, `native-tls`, `rustls` and `ring` are all absent),
  and the desktop build pulls in neither `reqwest` nor `hyper`: tauri only needs
  them for non-desktop targets.
- **External links**: only `http`/`https` URLs are handed to the OS.

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

- **lint**: `cargo fmt --check` over the whole tree (blocking)
- **build**: multi-platform build (Windows / macOS-x64 / macOS-arm64 / Linux) with
  npm + cargo caching, unit tests and advisory clippy on the Linux leg, and a hard
  failure when a leg produces no installer
- **release**: automated GitHub Release with checksums on tag push

The backend is built from `magicwubiao/go-magic` at `GO_MAGIC_REF` (workflow input
or repository variable, default `main`) — pin it to a tag or commit SHA for
reproducible builds.

macOS builds are signed and notarized automatically when the `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD` and `APPLE_TEAM_ID` secrets exist, and are skipped silently when
they do not.

## Versioning

Versions are derived **entirely from git tags** — no version field in any config
file is maintained manually. `package.json`, `Cargo.toml`, and `tauri.conf.json`
all keep a placeholder `0.0.0`.

At build time, `build.rs` runs `git describe --tags` to embed the real version
(plus git commit, branch, build time and build profile) into the binary. On tag
pushes, CI runs `scripts/sync-version.mjs <tag>` to stamp the same version into
`package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, so the
installer package carries it as well. Run the same script locally to preview it.

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