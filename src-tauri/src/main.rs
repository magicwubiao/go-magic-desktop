#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Go Magic Desktop - Process Isolation Mode
//!
//! This shell is deliberately thin: it spawns the bundled `go-magic` backend as
//! a child process, points a webview at `http://127.0.0.1:<port>` and shuts the
//! backend down again when the window closes.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

// ============================================================================
// Window State Management
// ============================================================================

const DEFAULT_WINDOW_WIDTH: f64 = 1024.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 800.0;
const MIN_WINDOW_WIDTH: f64 = 800.0;
const MIN_WINDOW_HEIGHT: f64 = 600.0;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
struct WindowState {
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            x: None,
            y: None,
        }
    }
}

/// Clamps a persisted window state to sizes the window manager will accept.
///
/// Also swallows `NaN` (a corrupt state file can deserialize to `f64::NAN`,
/// since JSON has no NaN literal but `1e400` and friends do).
fn sanitize_window_state(mut state: WindowState) -> WindowState {
    state.width = state.width.max(MIN_WINDOW_WIDTH);
    state.height = state.height.max(MIN_WINDOW_HEIGHT);
    state
}

fn get_window_state_path(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("window-state.json"))
}

fn adjust_position_for_screen(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    app_handle: &AppHandle,
) -> (f64, f64) {
    if let Some(monitor) = app_handle.primary_monitor().ok().flatten() {
        let scale = monitor.scale_factor();
        let wa = monitor.work_area();
        let screen_x = wa.position.x as f64 / scale;
        let screen_y = wa.position.y as f64 / scale;
        let screen_width = wa.size.width as f64 / scale;
        let screen_height = wa.size.height as f64 / scale;

        let mut new_x = x;
        let mut new_y = y;

        if new_x + width > screen_x + screen_width {
            new_x = screen_x + screen_width - width;
        }
        if new_x < screen_x {
            new_x = screen_x;
        }

        if new_y + height > screen_y + screen_height {
            new_y = screen_y + screen_height - height;
        }
        if new_y < screen_y {
            new_y = screen_y;
        }

        (new_x, new_y)
    } else {
        (x, y)
    }
}

fn save_window_state(app_handle: &AppHandle) {
    let Some(path) = get_window_state_path(app_handle) else {
        return;
    };
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };

    let scale_factor = window.scale_factor().unwrap_or(1.0);

    let (width, height) = match window.inner_size() {
        Ok(size) => {
            let logical: tauri::LogicalSize<u32> = size.to_logical(scale_factor);
            (logical.width as f64, logical.height as f64)
        }
        Err(_) => (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT),
    };

    let (x, y) = match window.outer_position() {
        Ok(pos) => {
            let logical: tauri::LogicalPosition<i32> = pos.to_logical(scale_factor);
            (Some(logical.x as f64), Some(logical.y as f64))
        }
        Err(_) => (None, None),
    };

    let state = WindowState {
        width: width.round(),
        height: height.round(),
        x: x.map(|v| v.round()),
        y: y.map(|v| v.round()),
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, &json) {
            eprintln!("Failed to save window state: {}", e);
        }
    }
}

fn load_window_state(app_handle: &AppHandle) -> WindowState {
    let Some(path) = get_window_state_path(app_handle) else {
        return WindowState::default();
    };
    if !path.exists() {
        return WindowState::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<WindowState>(&content) {
            Ok(state) => sanitize_window_state(state),
            Err(e) => {
                eprintln!("Failed to parse window state: {}", e);
                WindowState::default()
            }
        },
        Err(e) => {
            eprintln!("Failed to read window state: {}", e);
            WindowState::default()
        }
    }
}

// ============================================================================
// Constants Configuration
// ============================================================================

const DEFAULT_PORTS: &[u16] = &[5000, 5001, 5002, 5003, 5004, 8080, 3000];
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);
/// Timeout for a single health probe: connect, write and read alike.
const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Upper bound on how many bytes are read while hunting for the status line.
const MAX_STATUS_LINE_BYTES: usize = 1024;
/// How often the `backend-status` event is emitted while waiting for the backend.
const BACKEND_STATUS_INTERVAL: Duration = Duration::from_secs(1);
/// Grace period for the child process to die after being asked to terminate.
const BACKEND_STOP_TIMEOUT: Duration = Duration::from_secs(5);

// ============================================================================
// Backend Process Management
// ============================================================================

struct BackendState {
    process: Child,
    port: u16,
    start_time: Instant,
}

static BACKEND_STATE: Mutex<Option<BackendState>> = Mutex::new(None);

// --------------------------------------------------------------------------
// Port Management
// --------------------------------------------------------------------------

fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn pick_available_port() -> Option<u16> {
    for &port in DEFAULT_PORTS {
        if is_port_available(port) {
            return Some(port);
        }
    }
    (8000..9000).find(|&port| is_port_available(port))
}

// --------------------------------------------------------------------------
// Health Check
// --------------------------------------------------------------------------

/// Extracts the status code from an HTTP/1.x status line (`HTTP/1.1 200 OK`).
///
/// Anything that is not a status line — a TLS alert, a stray banner, an empty
/// response — yields `None`, which the caller treats as "not healthy".
fn parse_status_code(status_line: &str) -> Option<u16> {
    let mut parts = status_line.trim().split(' ');

    if !parts.next()?.starts_with("HTTP/") {
        return None;
    }

    parts.next()?.parse().ok()
}

/// Reads just enough of the response to recover the status code.
///
/// A single `read` may hand back a partial line, so this keeps reading until the
/// end of the status line or [`MAX_STATUS_LINE_BYTES`]. The response body is
/// never read: it cannot change the verdict and must not be buffered.
fn read_status_code(stream: &mut TcpStream) -> Option<u16> {
    let mut head = Vec::with_capacity(64);
    let mut chunk = [0u8; 64];

    loop {
        match stream.read(&mut chunk) {
            // EOF: take whatever arrived before the connection was closed.
            Ok(0) => break,
            Ok(read) => {
                head.extend_from_slice(&chunk[..read]);
                if head.contains(&b'\n') || head.len() >= MAX_STATUS_LINE_BYTES {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let head = String::from_utf8_lossy(&head);
    parse_status_code(head.lines().next().unwrap_or_default())
}

/// Probes `GET /health` on the local backend.
///
/// Speaks HTTP/1.1 over a raw socket on purpose: the only request this shell
/// ever makes is this plain-HTTP localhost liveness check, and a full HTTP client
/// (reqwest → hyper → tokio → rustls → ring) is a poor trade for one status line.
fn check_backend_health(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));

    let Ok(mut stream) = TcpStream::connect_timeout(&addr, HEALTH_PROBE_TIMEOUT) else {
        return false;
    };

    if stream.set_read_timeout(Some(HEALTH_PROBE_TIMEOUT)).is_err()
        || stream
            .set_write_timeout(Some(HEALTH_PROBE_TIMEOUT))
            .is_err()
    {
        return false;
    }

    // `Connection: close` keeps the backend from holding the socket open, so the
    // status line is flushed as part of the response.
    let request = format!(
        "GET /health HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         User-Agent: go-magic-desktop/{}\r\n\
         Accept: */*\r\n\
         Connection: close\r\n\r\n",
        env!("APP_VERSION")
    );

    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    read_status_code(&mut stream)
        .map(|code| (200..300).contains(&code))
        .unwrap_or(false)
}

/// Polls `/health` until the backend answers or [`HEALTH_CHECK_TIMEOUT`] elapses.
///
/// Intended to be called from a background thread: it blocks and emits
/// `backend-status` progress events while it waits.
fn wait_for_backend_ready(port: u16, app_handle: &AppHandle) -> bool {
    let start = Instant::now();

    #[cfg(debug_assertions)]
    println!("Waiting for backend on port {}...", port);

    let mut last_status = start;
    loop {
        // Throttled so a slow start does not flood the frontend with events.
        if last_status.elapsed() >= BACKEND_STATUS_INTERVAL {
            last_status = Instant::now();
            let _ = app_handle.emit(
                "backend-status",
                serde_json::json!({
                    "state": "starting",
                    "elapsed": start.elapsed().as_secs(),
                    "port": port
                }),
            );
        }

        if check_backend_health(port) {
            #[cfg(debug_assertions)]
            println!("Backend ready after {}ms", start.elapsed().as_millis());
            return true;
        }

        if start.elapsed() >= HEALTH_CHECK_TIMEOUT {
            break;
        }

        thread::sleep(HEALTH_CHECK_INTERVAL);
    }

    false
}

// --------------------------------------------------------------------------
// Process Control
// --------------------------------------------------------------------------

fn show_error_dialog(app_handle: &AppHandle, title: &str, message: &str) {
    eprintln!("{}: {}", title, message);
    app_handle
        .dialog()
        .message(message)
        .title(title)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

fn open_external_link(_app_handle: &AppHandle, url: &str) {
    // Only ever hand web URLs to the OS. Without this, a crafted link could ask
    // the shell to open `file://`, a custom scheme handler, or worse.
    let Ok(parsed) = url.parse::<tauri::Url>() else {
        eprintln!("Refusing to open unparsable URL: {}", url);
        return;
    };

    if !matches!(parsed.scheme(), "http" | "https") {
        eprintln!("Refusing to open non-http(s) URL: {}", url);
        return;
    }

    #[cfg(debug_assertions)]
    println!("Opening external link: {}", parsed);

    if let Err(e) = open::that(parsed.as_str()) {
        eprintln!("Failed to open external link {}: {}", parsed, e);
    }
}

/// Filesystem locations that may hold the backend binary, in priority order.
///
/// Both `resources/*` (which lands in `<resource_dir>/resources/`) and a
/// flattened layout (binary at the resource root) are covered, because the
/// packaging config and the bundler differ per platform.
fn backend_search_dirs(resource_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            dirs.push(exe_dir.to_path_buf());
            dirs.push(exe_dir.join("resources"));

            if let Some(app_dir) = exe_dir.parent() {
                dirs.push(app_dir.to_path_buf());
                dirs.push(app_dir.join("resources"));

                // macOS bundle layout: Foo.app/Contents/MacOS/bin -> Contents/Resources
                #[cfg(target_os = "macos")]
                dirs.push(app_dir.join("Resources"));
            }
        }
    }

    dirs.push(resource_dir.to_path_buf());
    // `bundle.resources`, e.g. `"resources/*"`, keeps the `resources/` prefix.
    dirs.push(resource_dir.join("resources"));

    dirs
}

fn find_backend_path(resource_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    const BINARY_NAMES: &[&str] = &["go-magic.exe", "go-magic"];
    #[cfg(not(target_os = "windows"))]
    const BINARY_NAMES: &[&str] = &["go-magic"];

    let dirs = backend_search_dirs(resource_dir);
    let mut searched: Vec<PathBuf> = Vec::new();

    for dir in &dirs {
        for name in BINARY_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                #[cfg(debug_assertions)]
                println!("Found backend: {:?}", candidate);
                return Some(candidate);
            }
            searched.push(candidate);
        }
    }

    // Last resort: let the OS resolve the binary from PATH.
    #[cfg(target_os = "windows")]
    const PATH_BINARY: &str = "go-magic.exe";
    #[cfg(not(target_os = "windows"))]
    const PATH_BINARY: &str = "go-magic";

    if Command::new(PATH_BINARY).arg("--version").output().is_ok() {
        #[cfg(debug_assertions)]
        println!("Found backend in PATH: {}", PATH_BINARY);
        return Some(PathBuf::from(PATH_BINARY));
    }

    eprintln!("Backend executable not found. Searched paths:");
    for (i, path) in searched.iter().enumerate() {
        eprintln!("  {}: {:?} (exists: {})", i + 1, path, path.exists());
    }

    None
}

#[derive(Debug)]
enum BackendError {
    NoPortAvailable,
    BackendNotFound,
    SpawnFailed(String),
    HealthCheckTimeout,
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::NoPortAvailable => write!(f, "No available port found"),
            BackendError::BackendNotFound => write!(f, "Backend executable not found"),
            BackendError::SpawnFailed(msg) => write!(f, "Failed to start backend: {}", msg),
            BackendError::HealthCheckTimeout => write!(
                f,
                "Backend health check timed out, please verify if backend is working correctly"
            ),
        }
    }
}

impl std::error::Error for BackendError {}

/// Continuously drains the child's stdout/stderr on dedicated threads.
///
/// Both streams must be read until EOF. Reading only the first N lines (as this
/// used to do) closes the pipe, which both loses the tail of the logs and — once
/// the OS pipe buffer fills up — blocks the backend forever on its next write.
fn drain_pipes(child: &mut Child) {
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                log::info!("[backend] {}", line);
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    log::warn!("[backend] {}", line);
                }
            }
        });
    }
}

/// Spawns the bundled backend **without** waiting for it to become healthy.
///
/// Keeping the spawn free of any blocking work is what lets `setup` create the
/// window immediately; the health check runs on a background thread instead.
fn spawn_backend(resource_dir: &Path) -> Result<(Child, u16), BackendError> {
    let port = pick_available_port().ok_or(BackendError::NoPortAvailable)?;

    #[cfg(debug_assertions)]
    println!("Selected port: {}", port);

    let backend_path = find_backend_path(resource_dir).ok_or(BackendError::BackendNotFound)?;

    #[cfg(debug_assertions)]
    println!("Backend path: {:?}", backend_path);

    let port_arg = port.to_string();

    let mut command = Command::new(&backend_path);
    command
        .args(["server", "--port", &port_arg])
        .env("GOMAGIC_PORT", &port_arg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Started from the resource directory so the backend resolves its own assets
    // relative to the working directory.
    #[cfg(not(target_os = "windows"))]
    command.current_dir(resource_dir);

    // A Rust backtrace is useful while developing and noise in a shipped build
    // (it would only leak internal paths into the log file).
    #[cfg(debug_assertions)]
    command.env("RUST_BACKTRACE", "1");

    // Don't flash a console window on Windows.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| BackendError::SpawnFailed(format!("{} ({})", e, backend_path.display())))?;

    #[cfg(debug_assertions)]
    println!("Backend process spawned, PID: {:?}", child.id());

    drain_pipes(&mut child);

    Ok((child, port))
}

/// Terminates the backend and reaps it.
fn stop_backend() {
    let Ok(mut guard) = BACKEND_STATE.lock() else {
        return;
    };
    let Some(mut state) = guard.take() else {
        return;
    };
    // Release the lock before waiting so a concurrent `stop_backend` cannot
    // block behind us while we sleep.
    drop(guard);

    #[cfg(debug_assertions)]
    println!(
        "Stopping backend (ran for {:.1}s)...",
        state.start_time.elapsed().as_secs_f64()
    );

    // On Windows the Go backend may have helper processes of its own; kill the
    // whole tree so nothing is left running behind the app's back.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = Command::new("taskkill")
            .args(["/PID", &state.process.id().to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    let _ = state.process.kill();

    // Reap the child, otherwise it stays around as a zombie.
    let deadline = Instant::now() + BACKEND_STOP_TIMEOUT;
    loop {
        match state.process.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = state.process.wait();
                break;
            }
        }
    }

    #[cfg(debug_assertions)]
    println!("Backend stopped");
}

/// Restarts the backend and waits for the replacement to answer.
///
/// Blocks; call it from a background thread (see [`restart_backend_cmd`]).
fn restart_backend(app_handle: &AppHandle, resource_dir: &Path) {
    #[cfg(debug_assertions)]
    println!("Restarting backend...");

    stop_backend();
    thread::sleep(Duration::from_secs(1));

    match spawn_backend(resource_dir) {
        Ok((process, port)) => {
            if let Ok(mut guard) = BACKEND_STATE.lock() {
                *guard = Some(BackendState {
                    process,
                    port,
                    start_time: Instant::now(),
                });
            }

            if !wait_for_backend_ready(port, app_handle) {
                let msg = format!(
                    "Backend did not become ready within {}s after a restart",
                    HEALTH_CHECK_TIMEOUT.as_secs()
                );
                eprintln!("{}", msg);
                let _ = app_handle.emit("backend-error", serde_json::json!({ "message": msg }));
                return;
            }

            // The new instance may have picked a different port, so point the
            // window at it again.
            if let Some(window) = app_handle.get_webview_window("main") {
                if let Ok(url) = format!("http://127.0.0.1:{}/", port).parse::<tauri::Url>() {
                    let _ = window.navigate(url);
                }
            }

            #[cfg(debug_assertions)]
            println!("Backend restarted on port {}", port);

            let _ = app_handle.emit("backend-restarted", port);
        }
        Err(e) => {
            eprintln!("Restart failed: {}", e);
            let _ = app_handle.emit(
                "backend-error",
                serde_json::json!({ "message": e.to_string() }),
            );
        }
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

#[tauri::command]
fn get_backend_port() -> Option<u16> {
    BACKEND_STATE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.port))
}

#[tauri::command]
fn restart_backend_cmd(app_handle: AppHandle) {
    let Ok(resource_dir) = app_handle.path().resource_dir() else {
        eprintln!("Restart failed: resource directory is unavailable");
        return;
    };

    // A restart takes seconds; never block the IPC thread on it.
    let _ = app_handle.emit("backend-restarting", serde_json::json!({}));
    thread::spawn(move || restart_backend(&app_handle, &resource_dir));
}

#[tauri::command]
fn get_app_info() -> serde_json::Value {
    serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("APP_VERSION"),
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "mode": "process-isolated",
        "git_commit": option_env!("GIT_COMMIT").unwrap_or(""),
        "git_branch": option_env!("GIT_BRANCH").unwrap_or(""),
        "build_time": option_env!("BUILD_TIME").unwrap_or(""),
        "build_profile": option_env!("BUILD_PROFILE").unwrap_or(""),
    })
}

#[tauri::command]
fn check_backend_health_cmd(port: Option<u16>) -> bool {
    let p = port.unwrap_or_else(|| {
        BACKEND_STATE
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.port))
            .unwrap_or(5000)
    });
    check_backend_health(p)
}

// ============================================================================
// Main Entry
// ============================================================================

fn main() {
    eprintln!(
        "Starting {} v{} (commit: {}, branch: {}, built: {}, profile: {})",
        env!("CARGO_PKG_NAME"),
        env!("APP_VERSION"),
        option_env!("GIT_COMMIT").unwrap_or("unknown"),
        option_env!("GIT_BRANCH").unwrap_or("unknown"),
        option_env!("BUILD_TIME").unwrap_or("unknown"),
        option_env!("BUILD_PROFILE").unwrap_or("unknown"),
    );

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                // The default level is `Trace`, which floods the log file. `Info`
                // keeps the lines that matter, including the backend's output.
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            let resource_dir = match app.path().resource_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    show_error_dialog(
                        &app_handle,
                        "Startup Error",
                        &format!("Failed to get resource directory: {}", e),
                    );
                    return Err(e.into());
                }
            };

            #[cfg(debug_assertions)]
            println!("Resource directory: {:?}", resource_dir);

            // Spawning is cheap, so do it up front and create the window right
            // after. The health check runs on a background thread: previously the
            // whole setup callback blocked on it for up to 60s, during which no
            // window existed at all.
            let (process, port) = match spawn_backend(&resource_dir) {
                Ok(result) => result,
                Err(e) => {
                    let error_msg = match &e {
                        BackendError::BackendNotFound => format!(
                            "Backend executable not found\nPlease check resource directory: {:?}",
                            resource_dir
                        ),
                        other => other.to_string(),
                    };

                    eprintln!("Failed to start backend: {}", error_msg);
                    show_error_dialog(&app_handle, "Application Startup Failed", &error_msg);
                    return Err("Backend startup failed".into());
                }
            };

            {
                let mut guard = BACKEND_STATE.lock().unwrap();
                *guard = Some(BackendState {
                    process,
                    port,
                    start_time: Instant::now(),
                });
            }

            let server_url = format!("http://127.0.0.1:{}/", port);
            let backend_url: tauri::Url = match server_url.parse() {
                Ok(url) => url,
                Err(e) => {
                    let msg = format!("Invalid backend URL {}: {}", server_url, e);
                    eprintln!("{}", msg);
                    show_error_dialog(&app_handle, "Application Startup Failed", &msg);
                    return Err("Invalid backend URL".into());
                }
            };

            #[cfg(debug_assertions)]
            println!("Creating window, URL: {}", server_url);

            let window_state = load_window_state(&app_handle);

            #[cfg(debug_assertions)]
            println!("Loaded window state: {:?}", window_state);

            let (x, y) = match (window_state.x, window_state.y) {
                (Some(x), Some(y)) => adjust_position_for_screen(
                    x,
                    y,
                    window_state.width,
                    window_state.height,
                    &app_handle,
                ),
                // No stored position yet: center on the primary monitor.
                _ => match app_handle.primary_monitor().ok().flatten() {
                    Some(monitor) => {
                        let scale = monitor.scale_factor();
                        let wa = monitor.work_area();
                        let screen_width = wa.size.width as f64 / scale;
                        let screen_height = wa.size.height as f64 / scale;
                        let screen_x = wa.position.x as f64 / scale;
                        let screen_y = wa.position.y as f64 / scale;
                        (
                            screen_x + (screen_width - window_state.width) / 2.0,
                            screen_y + (screen_height - window_state.height) / 2.0,
                        )
                    }
                    None => (0.0, 0.0),
                },
            };

            let window = match WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(backend_url.clone()),
            )
            .title("Go Magic")
            .inner_size(window_state.width, window_state.height)
            .min_inner_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
            .position(x, y)
            .focused(true)
            .resizable(true)
            .fullscreen(false)
            // Devtools are enabled in debug builds, and in release builds only
            // when the `devtools` cargo feature is opted into explicitly.
            .devtools(cfg!(any(debug_assertions, feature = "devtools")))
            .on_navigation({
                let app_handle_clone = app_handle.clone();
                move |url| {
                    let host = url.host_str().unwrap_or("");
                    // Allow local backend URLs
                    if host == "127.0.0.1" || host == "localhost" {
                        return true;
                    }
                    // Block other navigation and open in system browser instead
                    open_external_link(&app_handle_clone, url.as_str());
                    false
                }
            })
            .on_new_window({
                let app_handle_clone = app_handle.clone();
                move |url, _features| {
                    // Intercept target="_blank" links and open in system browser
                    let host = url.host_str().unwrap_or("");
                    // Allow local backend URLs
                    if host == "127.0.0.1" || host == "localhost" {
                        return NewWindowResponse::Allow;
                    }
                    // Open external links in system browser
                    open_external_link(&app_handle_clone, url.as_str());
                    NewWindowResponse::Deny
                }
            })
            .build()
            {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create window: {}", e);
                    return Err(e.into());
                }
            };

            #[cfg(any(debug_assertions, feature = "devtools"))]
            {
                window.open_devtools();
            }

            let _ = window.set_focus();

            // Wait for the backend off the UI thread, then make sure the window
            // is actually showing it. The window's very first load races the
            // backend and may briefly show a connection error; this reloads it
            // once the server answers.
            {
                let app_handle = app_handle.clone();
                let window = window.clone();

                thread::spawn(move || {
                    if wait_for_backend_ready(port, &app_handle) {
                        let _ = window.navigate(backend_url);

                        let _ = app_handle.emit(
                            "app-ready",
                            serde_json::json!({
                                "port": port,
                                "url": format!("http://127.0.0.1:{}", port)
                            }),
                        );

                        #[cfg(debug_assertions)]
                        println!("Application ready on port {}", port);
                    } else {
                        stop_backend();

                        let msg = format!(
                            "Backend did not become ready within {}s.\nPlease check the log file for details.",
                            HEALTH_CHECK_TIMEOUT.as_secs()
                        );
                        eprintln!("{}", msg);
                        let _ = app_handle
                            .emit("backend-error", serde_json::json!({ "message": msg }));
                        show_error_dialog(&app_handle, "Application Startup Failed", &msg);
                        app_handle.exit(1);
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_backend_port,
            restart_backend_cmd,
            get_app_info,
            check_backend_health_cmd,
        ])
        .on_window_event(|window, event| {
            let app_handle = window.app_handle().clone();
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    save_window_state(&app_handle);
                    stop_backend();
                    app_handle.exit(0);
                }
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                    save_window_state(&app_handle);
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_state_default_matches_constants() {
        let state = WindowState::default();
        assert_eq!(state.width, DEFAULT_WINDOW_WIDTH);
        assert_eq!(state.height, DEFAULT_WINDOW_HEIGHT);
        assert!(state.x.is_none());
        assert!(state.y.is_none());
    }

    #[test]
    fn window_state_is_clamped_to_the_minimum_size() {
        let tiny = sanitize_window_state(WindowState {
            width: 10.0,
            height: 20.0,
            x: Some(0.0),
            y: Some(0.0),
        });
        assert_eq!(tiny.width, MIN_WINDOW_WIDTH);
        assert_eq!(tiny.height, MIN_WINDOW_HEIGHT);

        // A corrupt state file must not be able to produce NaN geometry.
        let nan = sanitize_window_state(WindowState {
            width: f64::NAN,
            height: f64::NAN,
            x: None,
            y: None,
        });
        assert_eq!(nan.width, MIN_WINDOW_WIDTH);
        assert_eq!(nan.height, MIN_WINDOW_HEIGHT);

        let large = sanitize_window_state(WindowState {
            width: 4000.0,
            height: 3000.0,
            x: None,
            y: None,
        });
        assert_eq!(large.width, 4000.0);
        assert_eq!(large.height, 3000.0);
    }

    #[test]
    fn window_state_survives_a_json_round_trip() {
        let original = WindowState {
            width: 1280.0,
            height: 900.0,
            x: Some(12.0),
            y: Some(-34.0),
        };

        let json = serde_json::to_string(&original).unwrap();
        let restored: WindowState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.width, original.width);
        assert_eq!(restored.height, original.height);
        assert_eq!(restored.x, original.x);
        assert_eq!(restored.y, original.y);
    }

    #[test]
    fn pick_available_port_returns_a_port_from_the_expected_range() {
        let port = pick_available_port().expect("expected at least one free port");
        assert!(
            DEFAULT_PORTS.contains(&port) || (8000..9000).contains(&port),
            "unexpected port {}",
            port
        );
    }

    #[test]
    fn backend_errors_have_readable_messages() {
        assert_eq!(
            BackendError::NoPortAvailable.to_string(),
            "No available port found"
        );
        assert_eq!(
            BackendError::BackendNotFound.to_string(),
            "Backend executable not found"
        );
        assert_eq!(
            BackendError::SpawnFailed("boom".into()).to_string(),
            "Failed to start backend: boom"
        );
        assert!(BackendError::HealthCheckTimeout
            .to_string()
            .contains("timed out"));
    }

    #[test]
    fn health_check_timing_constants_are_sane() {
        assert!(HEALTH_CHECK_INTERVAL < HEALTH_CHECK_TIMEOUT);
        assert!(BACKEND_STATUS_INTERVAL >= HEALTH_CHECK_INTERVAL);
        assert!(BACKEND_STOP_TIMEOUT <= HEALTH_CHECK_TIMEOUT);
        assert!(HEALTH_PROBE_TIMEOUT < HEALTH_CHECK_TIMEOUT);
        assert!(HEALTH_CHECK_TIMEOUT >= Duration::from_secs(10));
    }

    #[test]
    fn parse_status_code_reads_the_code() {
        assert_eq!(parse_status_code("HTTP/1.1 200 OK"), Some(200));
        assert_eq!(parse_status_code("HTTP/1.0 204 No Content"), Some(204));
        assert_eq!(
            parse_status_code("HTTP/1.1 503 Service Unavailable\r"),
            Some(503)
        );
    }

    #[test]
    fn parse_status_code_rejects_everything_else() {
        // A port held by something that is not our backend must read as unhealthy.
        assert_eq!(parse_status_code(""), None);
        assert_eq!(parse_status_code("SSH-2.0-OpenSSH_9.6"), None);
        assert_eq!(parse_status_code("HTTP/1.1"), None);
        assert_eq!(parse_status_code("HTTP/1.1 abc Not a Number"), None);
    }

    /// Serves one canned response on an ephemeral port, then runs the real probe
    /// against it: end-to-end coverage without a live backend.
    fn probe_against(response: &'static str) -> bool {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut request = [0u8; 256];
                let _ = socket.read(&mut request);
                let _ = socket.write_all(response.as_bytes());
            }
        });

        let healthy = check_backend_health(port);
        server.join().expect("server thread");
        healthy
    }

    #[test]
    fn health_probe_accepts_2xx_and_rejects_anything_else() {
        assert!(probe_against(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
        ));
        assert!(!probe_against(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"
        ));
        // Nothing at all (a socket that accepts and stays silent) must not hang
        // forever either; the read timeout turns it into "unhealthy".
        assert!(!probe_against(""));
    }

    #[test]
    fn health_probe_reports_a_closed_port_as_unhealthy() {
        // Bind and immediately drop, so the port is (almost certainly) free.
        let port = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };

        assert!(!check_backend_health(port));
    }

    #[test]
    fn backend_search_dirs_cover_the_resource_subdirectory() {
        let resource_dir = Path::new("some").join("resource-dir");
        let dirs = backend_search_dirs(&resource_dir);

        assert!(dirs.contains(&resource_dir));
        // `bundle.resources = ["resources/*"]` keeps its prefix, so the binary
        // ends up one level deeper than the resource root.
        assert!(dirs.contains(&resource_dir.join("resources")));
    }
}
