# Go Magic Desktop

> **A Tauri-powered desktop shell for [Go Magic](https://github.com/magicwubiao/go-magic).**

[English](README.md) | [中文](README.zh-CN.md)

Go Magic Desktop wraps the Go Magic web application into a native, cross-platform
desktop app. It bundles the `go-magic` backend, manages it as a child process, and
renders its web UI inside a lightweight Tauri WebView — no browser tab, no manual
server setup, no extra configuration.

---

## Table of Contents

- [Why a Desktop App?](#why-a-desktop-app)
- [Highlights](#highlights)
- [Getting Started](#getting-started)
- [Using the App](#using-the-app)
- [Project Layout](#project-layout)
- [Prerequisites](#prerequisites)
- [Development](#development)
- [Building & Packaging](#building--packaging)
- [How It Works](#how-it-works)
- [Configuration](#configuration)
- [Security Model](#security-model)
- [Logs & Troubleshooting](#logs--troubleshooting)
- [CI/CD & Releases](#cicd--releases)
- [Versioning](#versioning)
- [License & Contributing](#license--contributing)

---

## Why a Desktop App?

`go-magic` is a standalone Go program, not a library. Rather than re-implementing
its backend or forking its source, this project treats it as a **black box** and
supervises it as a child process. That design buys three things:

- **Shared backend** — the CLI and the desktop app run the exact same code.
- **Zero source changes** — go-magic is never modified.
- **Effortless upgrades** — swap in a newer binary and you're done.

## Highlights

| Area | What you get |
|------|--------------|
| **Cross-platform** | Windows · macOS (Intel & Apple Silicon) · Linux |
| **Embedded backend** | go-magic is auto-started, supervised, and shut down with the app |
| **Instant window** | The UI appears immediately; health checks run on a background thread (no 60s startup freeze) |
| **Smart ports** | Conflicts avoided via automatic port selection |
| **Robust process mgmt** | stdout/stderr drained continuously (no pipe deadlock), child reaped on exit, whole process tree killed on Windows |
| **State persistence** | Window position, size, and maximized state restored across sessions |
| **Hi-DPI aware** | Correct window sizing and placement under display scaling |
| **External links** | `http`/`https` links open in the system browser; local ones stay in-app |
| **In-app restart** | Restart the backend without closing the app or blocking the UI |
| **Runtime app info** | Version, git commit, branch, and build metadata at your fingertips |
| **Structured logging** | Logs go to stdout **and** a file, covering both the shell and the backend |
| **Hardened security** | Least-privilege capabilities, strict CSP, no TLS stack, no devtools in release |

---

## Getting Started

### 1. Download

Grab the installer for your platform from the
[Releases](https://github.com/magicwubiao/go-magic-desktop/releases) page:

| Platform | Installer |
| --- | --- |
| Windows | `.exe` (NSIS) |
| macOS | `.dmg` (Intel & Apple Silicon) |
| Linux | `.AppImage` · `.deb` |

### 2. Launch

Open the app (window title: **"Go Magic"**). On startup it automatically:

1. Locates the bundled `go-magic` backend executable.
2. Picks an available port (default `5000`, falling back through `5001`/`5002`/…/`3000`).
3. Shows the window **immediately** while the backend starts in the background
   (health check budget: up to 60 seconds).
4. Loads the Go Magic web UI as soon as the backend answers.

### 3. Use it

Everything runs inside the window — the standard Go Magic web interface, no setup
required.

---

## Using the App

### Backend management

- The backend is **fully managed** — it starts with the app and stops when you close the window.
- **Restart from the UI**: if the backend becomes unresponsive, trigger the in-app restart (`restart_backend_cmd`) — no need to quit.
- **App info**: query version, git commit, and build metadata at any time via `get_app_info`.

### Links & navigation

- Links to `127.0.0.1` / `localhost` open **inside** the app.
- Any **external** link opens in your **system default browser**.

### Window & layout

- Default size is **1024×800** (minimum **800×600**).
- Position and size are **persisted across sessions** and restored on next launch, with Hi-DPI adaptation.
- A **maximized** window is restored as maximized: only the flag is remembered, while the underlying geometry stays at its pre-maximize size. (Persisting the maximized geometry instead would restore a monitor-sized *normal* window that lands a few pixels short because of the frame border.)

### Exiting

Closing the window quits the app — it saves the window state and gracefully stops the backend.

---

## Project Layout

```
go-magic-desktop/
├── src-tauri/                  # Rust shell + backend supervision
│   ├── src/main.rs             # Entry point (process management, window)
│   ├── build.rs                # Embeds version / commit / build-time metadata
│   ├── Cargo.toml              # Rust dependencies
│   ├── tauri.conf.json         # Tauri config (window, CSP, bundling)
│   ├── capabilities/           # Least-privilege permission sets
│   ├── icons/                  # Application icons
│   └── resources/              # Bundled backend binary (populated at build time)
├── scripts/sync-version.mjs    # Stamps the git tag into all version fields
├── .github/workflows/build.yml # Multi-platform CI + release
└── package.json                # Node.js configuration
```

---

## Prerequisites

### Required software

| Tool | Version | Notes |
| --- | --- | --- |
| **Rust** | 1.77.2+ | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` |
| **Node.js** | 18+ | |
| **Tauri CLI** | — | `npm install -g @tauri-apps/cli` |
| **Go** | 1.22+ | For building the backend — https://go.dev/dl/ |

### System dependencies

- **Windows** — Microsoft Visual Studio C++ Build Tools, WebView2 Runtime.
- **macOS** — Xcode Command Line Tools.
- **Linux** —
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf
  ```
  No OpenSSL development package is needed: neither the shell nor the Tauri crates
  it depends on link a TLS backend, so `openssl-sys`, `native-tls`, and `rustls`
  never enter the build.

---

## Development

```bash
# 1. Enter the project
cd go-magic-desktop

# 2. Install JS dependencies
npm install

# 3. Clone the Go Magic main repo (into the same parent dir)
cd ../ && git clone https://github.com/magicwubiao/go-magic.git
cd go-magic-desktop

# 4. Build the Go Magic backend into the resources dir
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# 5. Run in dev mode
npm run dev
```

### Tests, formatting & linting

```bash
npm test               # cargo test — unit tests for pure helpers
npm run fmt            # cargo fmt
npm run clippy         # cargo clippy --all-targets -- -D warnings
npm run version:sync   # stamp current git tag into package.json / Cargo.toml / tauri.conf.json
```

CI enforces `cargo fmt --check` in a dedicated lint job. `cargo clippy` runs in
**advisory** mode — drop `continue-on-error` in `.github/workflows/build.yml` once
the tree is clippy-clean.

---

## Building & Packaging

### Quick build

```bash
npm run build          # build frontend + package the desktop app
npm run build:linux    # or target a specific platform
```

### Step-by-step build

```bash
# 1. Build the frontend
cd ../go-magic/web && npm run build

# 2. Build the backend
cd ../go-magic && go build -o ../go-magic-desktop/src-tauri/resources/go-magic ./cmd/magic

# 3. Package the desktop app
cd ../go-magic-desktop && tauri build
```

### Multi-platform build

Tauri apps aren't practical to cross-compile, so each platform is built on its own
runner (or machine) via the npm scripts:

```bash
npm run build:windows    # x86_64-pc-windows-msvc
npm run build:macos      # x86_64-apple-darwin
npm run build:macos-arm  # aarch64-apple-darwin
npm run build:linux      # x86_64-unknown-linux-gnu
```

CI mirrors this for all four targets — just push a tag.

### Build output

Installers land in:

| Platform | Location |
| --- | --- |
| Windows NSIS | `src-tauri/target/release/bundle/nsis/*.exe` |
| macOS | `src-tauri/target/release/bundle/dmg/*.dmg` |
| Linux AppImage | `src-tauri/target/release/bundle/appimage/*.AppImage` |
| Linux DEB | `src-tauri/target/release/bundle/deb/*.deb` |

---

## How It Works

### Go Magic integration

The Tauri app orchestrates the whole lifecycle automatically:

1. **Detect** the go-magic executable in the `resources` directory.
2. **Start** `go-magic server --port <PORT>`.
3. **Health-check** — wait for readiness (60s budget, then show an error and exit).
4. **Load** `http://127.0.0.1:<PORT>/` in the WebView.
5. **Show** the main window and focus it.
6. **Shut down** the backend on window close.

### Port priority

Auto-selected available ports: `5000 → 5001 → 5002 → 5003 → 5004 → 8080 → 3000`.

### Process model

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
│  │   (standalone subprocess)   │   │
│  └─────────────┬───────────────┘   │
│                │                    │
│  ┌─────────────▼───────────────┐   │
│  │   WebView (user interface)  │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
```

### Tauri commands

| Command | Purpose |
| --- | --- |
| `get_backend_port` | Get the port the backend is running on |
| `restart_backend_cmd` | Restart the backend process |
| `get_app_info` | Get version, git commit, build time, and profile |
| `check_backend_health_cmd` | Check whether the backend is healthy |

---

## Configuration

### Tauri configuration

Edit `tauri.conf.json` to adjust window size/title, permission settings, and
packaging options.

### Environment variables

| Variable | Description |
| --- | --- |
| `GOMAGIC_PORT` | Backend listening port (also passed as `--port`) |
| `RUST_BACKTRACE` | Set to `1` for the backend **in debug builds only** |

Bundle-time metadata (`APP_VERSION`, `GIT_COMMIT`, `GIT_BRANCH`, `BUILD_TIME`,
`BUILD_PROFILE`) is injected by `src-tauri/build.rs` and surfaced through
`get_app_info`.

---

## Security Model

- **CSP** — script/style/connect targets are restricted to the app itself and the
  local backend, plus `object-src 'none'`, `base-uri 'self'`, and
  `frame-ancestors 'none'`. `'unsafe-eval'` is disallowed; `'unsafe-inline'` is
  still required by the bundled UI's inline script/style, and `img-src` also allows
  `https:` and `data:`.
- **Port binding** — the backend only ever binds `127.0.0.1`.
- **Permissions** — `src-tauri/capabilities/default.json` grants only `core:default`,
  and only to **local** content. The Go Magic UI is served by the bundled backend
  over `http://127.0.0.1:<port>`, which Tauri classifies as *remote* content — so
  the UI deliberately gets **no** access to Tauri commands or plugins. If that
  origin ever needs IPC, add an explicit `remote` capability rather than widening
  the default one.
- **No devtools in release** — the `devtools` cargo feature is off by default, and
  the window builder disables devtools when `debug_assertions` is off.
- **TLS-free** — the shell brings no HTTP client of its own; its only request
  (`GET /health` on `127.0.0.1`) goes over a raw socket. `Cargo.lock` carries no TLS
  crate at all (`openssl`, `native-tls`, `rustls`, `ring` are all absent), and the
  desktop build pulls in neither `reqwest` nor `hyper` — Tauri only needs them for
  non-desktop targets.
- **External links** — only `http`/`https` URLs are handed to the OS.

---

## Logs & Troubleshooting

### Log locations

- **Windows** — `%APPDATA%/go-magic-desktop/logs/`
- **macOS** — `~/Library/Logs/go-magic-desktop/`
- **Linux** — `~/.local/share/go-magic-desktop/logs/`

### Log files

- `go-magic.log` — application main log
- `go-magic.log.0` — rotated log

### FAQ

**WebView2 missing (Windows)**
```powershell
winget install Microsoft.WebView2
```

**Backend fails to start**
1. Check that `src-tauri/resources/go-magic.exe` exists.
2. Review the error messages in the log files.
3. Verify the port isn't occupied.

**macOS security warning**
System Preferences → Security & Privacy → Allow anyway.

---

## CI/CD & Releases

Automated builds run on GitHub Actions:

- **lint** — `cargo fmt --check` over the whole tree (blocking).
- **build** — multi-platform (Windows / macOS-x64 / macOS-arm64 / Linux) with npm +
  cargo caching, unit tests and advisory clippy on the Linux leg, and a hard failure
  when a leg produces no installer.
- **release** — automated GitHub Release with checksums on tag push.

The backend is built from `magicwubiao/go-magic` at `GO_MAGIC_REF` (workflow input
or repository variable, default `main`) — pin it to a tag or commit SHA for
reproducible builds.

macOS builds are signed and notarized automatically when the `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD`, and `APPLE_TEAM_ID` secrets exist, and are skipped silently when
they don't.

---

## Versioning

Versions are derived **entirely from git tags** — no version field in any config
file is maintained by hand. `package.json`, `Cargo.toml`, and `tauri.conf.json`
all keep a placeholder `0.0.0`.

At build time, `build.rs` runs `git describe --tags` to embed the real version
(plus git commit, branch, build time, and profile) into the binary. On tag pushes,
CI runs `scripts/sync-version.mjs <tag>` to stamp the same version into
`package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, so the
installer carries it too. Run the same script locally to preview.

### Release flow

```bash
git tag v1.0.0
git push origin v1.0.0
```

CI then builds for Windows, macOS (x64 + arm64), and Linux, and creates a GitHub
Release with checksums.

### Local builds

Builds without a tag produce version `0.0.0-dev`. To build with a specific version,
create a tag first:

```bash
git tag v0.5.1
npm run build
```

---

## License & Contributing

- **License**: MIT
- **Contributing**: Issues and Pull Requests are welcome!