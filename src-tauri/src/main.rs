#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Go Magic Desktop - Process Isolation Mode
//!
//! This shell is deliberately thin: it spawns the bundled `go-magic` backend as
//! a child process, points a webview at `http://127.0.0.1:<port>` and shuts the
//! backend down again when the window closes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, TryLockError};
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
    /// Whether the window was maximized when the app was last closed.
    ///
    /// `width`/`height` deliberately stay pinned to the *restored* (pre-maximize)
    /// size, because a maximized window reports the monitor's size and restoring
    /// that as a plain window would leave it a few pixels short of the screen
    /// (the frame border) — which is exactly the "nearly full screen but
    /// offset" bug. `serde(default)` keeps older state files loading.
    #[serde(default)]
    maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            x: None,
            y: None,
            maximized: false,
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

    // A maximized (or fullscreen) window reports the monitor's size in both
    // `inner_size` and `outer_position`, so persisting those would destroy the
    // restore geometry. Keep the previous size instead and only record the
    // `maximized` flag — that is what makes "maximize → close → reopen" land
    // exactly on the monitor again instead of a few pixels off.
    let maximized = window.is_maximized().unwrap_or(false);
    let fullscreen = window.is_fullscreen().unwrap_or(false);

    if maximized || fullscreen {
        let mut state = load_window_state(app_handle);
        state.maximized = maximized || fullscreen;
        write_window_state(&path, &state);
        return;
    }

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
        maximized: false,
    };

    write_window_state(&path, &state);
}

fn write_window_state(path: &Path, state: &WindowState) {
    if let Ok(json) = serde_json::to_string_pretty(state) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(path, &json) {
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

/// Reveals `window` on screen already maximized, in a single step.
///
/// tao maximizes a window that is not visible yet with `ShowWindow(SW_MAXIMIZE)`
/// and then hides it again, which makes the window appear for an instant and
/// vanish (see the comment on the window builder in `main`). Asking the window
/// manager for `SW_SHOWMAXIMIZED` instead keeps the whole job in one call: the
/// first frame that is ever composited is already the final, maximized one, and
/// the window manager computes the frame and the monitor work area itself — so
/// this does not reintroduce the "monitor-sized rectangle, a few pixels off"
/// bug that persisting the maximized geometry used to cause.
///
/// Returns `false` when the native call could not be made, so the caller can
/// fall back to tao's own `maximize()`.
///
/// Once the tauri in use ships with tao >= 0.37.0 this helper and the
/// `cfg(not(windows))` split in `main` can both be dropped in favour of a plain
/// `maximized(window_state.maximized)` on the builder.
#[cfg(windows)]
fn reveal_maximized<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    // `ShowWindow` command: activates the window and displays it maximized.
    const SW_SHOWMAXIMIZED: i32 = 3;

    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(hwnd: *mut std::ffi::c_void, cmd_show: i32) -> i32;
    }

    let Ok(hwnd) = window.hwnd() else {
        return false;
    };

    // SAFETY: `hwnd` is the handle of the window that was just created and is
    // still alive here, and `ShowWindow` has no preconditions beyond the handle
    // being valid.
    unsafe {
        ShowWindow(hwnd.0, SW_SHOWMAXIMIZED);
    }

    true
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
/// How often the supervisor checks whether the backend is still running.
const SUPERVISE_INTERVAL: Duration = Duration::from_secs(1);
/// Consecutive automatic restart attempts before the supervisor gives up.
///
/// Reset as soon as the backend survives a full [`SUPERVISE_INTERVAL`], so a
/// one-off crash hours into a session does not consume the whole budget.
const MAX_BACKEND_RESTARTS: u32 = 5;
/// First backoff step; doubled on every further failed attempt.
const AUTO_RESTART_BASE_DELAY: Duration = Duration::from_secs(1);
/// Ceiling for the exponential backoff.
const AUTO_RESTART_MAX_DELAY: Duration = Duration::from_secs(30);
/// How many stderr lines are kept around to explain an unexpected exit.
const BACKEND_STDERR_TAIL_LINES: usize = 20;

// ============================================================================
// Backend Process Management
// ============================================================================

struct BackendState {
    process: Child,
    port: u16,
    start_time: Instant,
}

static BACKEND_STATE: Mutex<Option<BackendState>> = Mutex::new(None);
/// Incremented every time a new backend process is installed.
///
/// The startup health check uses it to tell "my backend never answered" from
/// "the supervisor already replaced it", which need opposite reactions.
static BACKEND_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Set once the app is going away. The supervisor must never resurrect a
/// backend that was stopped on purpose.
static BACKEND_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
/// Serialises manual and automatic restarts: two concurrent restarts would
/// spawn two backends that then fight over the port and `sessions.db`.
static RESTART_GUARD: Mutex<()> = Mutex::new(());
/// Last stderr lines of the current child, kept so a crash can be explained.
///
/// The desktop log already carries everything via `tauri-plugin-log`, but a
/// panic is printed once and then drowned by request logging; repeating the
/// tail next to the exit status makes the cause impossible to miss.
static BACKEND_STDERR_TAIL: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

// --------------------------------------------------------------------------
// Process Bookkeeping
// --------------------------------------------------------------------------

/// Installs a freshly spawned backend, replacing whatever was registered before.
fn install_backend(process: Child, port: u16) {
    // A poisoned lock must not swallow the new child: dropping the guard would
    // leak the freshly spawned process (std does not kill on drop) while the
    // state still looked empty, and the supervisor would then restart forever.
    let mut guard = match BACKEND_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    *guard = Some(BackendState {
        process,
        port,
        start_time: Instant::now(),
    });
    drop(guard);

    // Bumped only after the new child is in place, so a reader that observes a
    // new generation also observes the matching process.
    BACKEND_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn backend_generation() -> u64 {
    BACKEND_GENERATION.load(Ordering::SeqCst)
}

/// True while the registered backend process is still running.
///
/// `Err` counts as running: an unreadable child is not evidence of a crash, and
/// guessing "dead" here would make the app quit on a transient OS error.
fn backend_child_running() -> bool {
    let mut guard = match BACKEND_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    match guard.as_mut() {
        Some(state) => !matches!(state.process.try_wait(), Ok(Some(_))),
        None => false,
    }
}

/// Describes an exit status in the terms that matter for a crash report:
/// an exit code, or the signal that killed the process.
///
/// The signal is the useful half: `9` (SIGKILL) points at the OOM killer or a
/// stray `kill -9`, `15` at a `SIGTERM` from a restart script, while a plain
/// exit code `2` is Go's signature for an unrecovered panic.
fn describe_exit_status(status: ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal() {
            return format!("killed by signal {} ({})", signal, signal_name(signal));
        }
    }

    match status.code() {
        Some(code) => format!("exit code {}", code),
        None => status.to_string(),
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> &'static str {
    match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        10 => "SIGUSR1",
        11 => "SIGSEGV",
        12 => "SIGUSR2",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => "unknown signal",
    }
}

/// Exponential backoff for automatic restarts: 1s, 2s, 4s, 8s, 16s, then 30s.
fn restart_delay(attempt: u32) -> Duration {
    // Cap the shift so a large attempt counter cannot overflow the multiplier.
    let factor = 1u64 << attempt.min(5);
    let millis = (AUTO_RESTART_BASE_DELAY.as_millis() as u64).saturating_mul(factor);

    Duration::from_millis(millis).min(AUTO_RESTART_MAX_DELAY)
}

/// Sleeps for `total`, but returns `true` as soon as the app starts shutting
/// down — a quit must not wait out a 30s backoff.
fn sleep_unless_shutting_down(total: Duration) -> bool {
    const SLICE: Duration = Duration::from_millis(250);

    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if BACKEND_SHUTTING_DOWN.load(Ordering::SeqCst) {
            return true;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        thread::sleep(SLICE.min(remaining));
    }

    BACKEND_SHUTTING_DOWN.load(Ordering::SeqCst)
}

fn remember_stderr_line(line: &str) {
    let mut tail = BACKEND_STDERR_TAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if tail.len() >= BACKEND_STDERR_TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line.to_string());
}

fn clear_stderr_tail() {
    BACKEND_STDERR_TAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

fn backend_stderr_tail() -> Vec<String> {
    BACKEND_STDERR_TAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// What the supervisor knows about a backend that is no longer there.
struct BackendExit {
    status: String,
    uptime: Duration,
    port: u16,
}

/// Reports the registered backend as exited, or `None` while it is still alive.
///
/// A missing registration counts as an exit on purpose: it is what a failed
/// restart leaves behind, and the supervisor must retry that case too.
fn unexpected_backend_exit() -> Option<BackendExit> {
    let mut guard = match BACKEND_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let Some(state) = guard.as_mut() else {
        return Some(BackendExit {
            status: "no backend process is running".to_string(),
            uptime: Duration::ZERO,
            port: 0,
        });
    };

    match state.process.try_wait() {
        Ok(Some(status)) => Some(BackendExit {
            status: describe_exit_status(status),
            uptime: state.start_time.elapsed(),
            port: state.port,
        }),
        Ok(None) => None,
        Err(e) => {
            log::warn!("[backend] could not query the backend process: {}", e);
            None
        }
    }
}



// --------------------------------------------------------------------------
// Port Management
// --------------------------------------------------------------------------

/// Probes the port the same way the backend will bind it.
///
/// The Go backend listens on the wildcard (`:%d`), so the probe must claim the
/// wildcard too. On Windows a loopback-specific bind (`127.0.0.1:port`)
/// *succeeds* even while another process already owns the wildcard port — Go
/// does not set `SO_EXCLUSIVEADDRUSE` — so probing loopback let two app
/// instances both "verify" port 5000 as free. The second backend then died on
/// startup, and because the surviving instance's server kept answering the
/// health probe, the supervisor treated every restart as a success and
/// reloaded the window every few seconds, forever.
fn is_port_available(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok()
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
/// Note the probe only sees *a* server on this port — it cannot attribute the
/// answer to this instance's child. Callers that care (every caller does) must
/// pair a successful probe with [`backend_child_running`].
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
/// `generation` names the backend process the caller is waiting for. When the
/// supervisor replaces that process mid-wait this returns `false` immediately:
/// the supervisor navigates the window itself, and a caller that kept polling
/// the old port would later reload a page the user is already working in.
/// Pass `None` from a restart — the caller *is* the replacement.
///
/// Intended to be called from a background thread: it blocks and emits
/// `backend-status` progress events while it waits.
fn wait_for_backend_ready(port: u16, generation: Option<u64>, app_handle: &AppHandle) -> bool {
    let start = Instant::now();

    #[cfg(debug_assertions)]
    println!("Waiting for backend on port {}...", port);

    let mut last_status = start;
    loop {
        if matches!(generation, Some(expected) if backend_generation() != expected) {
            #[cfg(debug_assertions)]
            println!("Backend was replaced while waiting; handing over to the supervisor");

            return false;
        }

        // A child that is gone cannot become ready. Bail out instead of burning
        // the full timeout: a crash-looping backend is then retried within
        // seconds per attempt rather than a minute per attempt.
        if !backend_child_running() {
            return false;
        }

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
            // A 200 is only meaningful if it came from OUR child. While two
            // app instances are running, the other instance's backend may be
            // the one answering here (e.g. both selected the same port before
            // either bound it): the probe cannot tell the servers apart.
            // Reporting success for a dead child reset the supervisor's retry
            // budget on every attempt and reloaded the window forever, so a
            // dead child always counts as "not ready".
            if !backend_child_running() {
                #[cfg(debug_assertions)]
                println!("Health probe answered but the backend child is gone; treating as not ready");

                return false;
            }

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
                if line.trim().is_empty() {
                    continue;
                }

                remember_stderr_line(&line);

                // A Go panic / runtime fatal error is printed exactly once, as
                // the last thing the process says. Logging it at error level
                // (and keeping it in the tail) is what turns a silent "the
                // service closed" into an actionable stack trace.
                if line.contains("panic:") || line.contains("fatal error:") {
                    log::error!("[backend] {}", line);
                } else {
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

    // The tail describes one child's lifetime; a new child starts with a clean
    // slate so a later crash report cannot quote an older process's output.
    clear_stderr_tail();

    let mut child = command
        .spawn()
        .map_err(|e| BackendError::SpawnFailed(format!("{} ({})", e, backend_path.display())))?;

    log::info!(
        "[backend] spawned PID {:?} on port {} ({})",
        child.id(),
        port,
        backend_path.display()
    );

    #[cfg(debug_assertions)]
    println!("Backend process spawned, PID: {:?}", child.id());

    drain_pipes(&mut child);

    Ok((child, port))
}

/// Terminates the backend and reaps it.
fn stop_backend() {
    // Recover from a poisoned lock instead of bailing out: leaving the child
    // running would keep it bound to the port after the app is gone.
    let mut guard = match BACKEND_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
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

/// Stops the backend because the app is going away.
///
/// The flag is what keeps the supervisor from resurrecting a backend that was
/// stopped on purpose: once the child was gone, closing the window and a crash
/// looked exactly the same.
fn shutdown_backend() {
    BACKEND_SHUTTING_DOWN.store(true, Ordering::SeqCst);
    stop_backend();
}

/// Restarts the backend and waits for the replacement to answer.
///
/// Returns `true` when a healthy replacement is running. Blocks for up to
/// [`HEALTH_CHECK_TIMEOUT`], so call it from a background thread (see
/// [`restart_backend_cmd`] and [`supervise_backend`]).
fn restart_backend(app_handle: &AppHandle, resource_dir: &Path) -> bool {
    #[cfg(debug_assertions)]
    println!("Restarting backend...");

    log::info!("[backend] restart requested");

    stop_backend();

    // A restart queued just before the window closed must not spawn a process
    // that then outlives the app.
    if BACKEND_SHUTTING_DOWN.load(Ordering::SeqCst) {
        return false;
    }

    thread::sleep(Duration::from_secs(1));

    let (process, port) = match spawn_backend(resource_dir) {
        Ok(spawned) => spawned,
        Err(e) => {
            log::error!("[backend] restart failed: {}", e);
            let _ = app_handle.emit(
                "backend-error",
                serde_json::json!({ "message": e.to_string() }),
            );
            return false;
        }
    };

    install_backend(process, port);

    if !wait_for_backend_ready(port, None, app_handle) {
        let msg = format!(
            "Backend did not become ready within {}s after a restart",
            HEALTH_CHECK_TIMEOUT.as_secs()
        );
        log::error!("[backend] {}", msg);
        let _ = app_handle.emit("backend-error", serde_json::json!({ "message": msg }));

        // A replacement that never answers is no better than a dead one. Drop
        // it so the supervisor sees "no backend" on its next tick and retries
        // with backoff, instead of leaving a half-started process behind.
        stop_backend();
        return false;
    }

    // The new instance may have picked a different port, so point the window at
    // it again.
    if let Some(window) = app_handle.get_webview_window("main") {
        if let Ok(url) = format!("http://127.0.0.1:{}/", port).parse::<tauri::Url>() {
            let _ = window.navigate(url);
        }
    }

    log::info!("[backend] restarted on port {}", port);

    #[cfg(debug_assertions)]
    println!("Backend restarted on port {}", port);

    let _ = app_handle.emit("backend-restarted", port);

    true
}

/// Watches the backend child and brings it back when it dies.
///
/// The shell used to spawn the backend exactly once. A panic in any goroutine
/// (Go takes the whole process down, even when `net/http` would have recovered
/// the same panic inside a handler), an OOM kill, or a stray `kill` therefore
/// left the window pointing at a closed port: no message, no recovery short of
/// restarting the app. The supervisor turns that into a logged, self-healing
/// restart — and, once the retry budget is spent, into a visible error instead
/// of a silently dead window.
///
/// It deliberately restarts on *process exit* only, never on a failed `/health`
/// probe: a busy backend can miss a probe while a long-running task is making
/// progress, and killing it there would throw the user's work away.
fn supervise_backend(app_handle: AppHandle, resource_dir: PathBuf) {
    let mut failures: u32 = 0;

    loop {
        thread::sleep(SUPERVISE_INTERVAL);

        if BACKEND_SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }

        // Only one restart may be in flight. A manual restart (or a previous
        // auto-restart) holds this guard, so a tick landing in the middle of it
        // must not conclude that the backend is gone.
        let _guard = match RESTART_GUARD.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => continue,
        };

        if BACKEND_SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }

        let Some(exit) = unexpected_backend_exit() else {
            // Alive for a full interval: the previous crash is behind us, so the
            // next one gets the full retry budget again.
            failures = 0;
            continue;
        };

        log::error!(
            "[backend] exited unexpectedly after {:.1}s on port {}: {}",
            exit.uptime.as_secs_f64(),
            exit.port,
            exit.status
        );

        // The exit status says *that* it died; the tail says *why*.
        for line in backend_stderr_tail() {
            log::error!("[backend] (last output) {}", line);
        }

        let _ = app_handle.emit(
            "backend-exited",
            serde_json::json!({
                "status": exit.status.clone(),
                "port": exit.port,
                "uptime": exit.uptime.as_secs(),
            }),
        );

        if failures >= MAX_BACKEND_RESTARTS {
            let msg = format!(
                "The Go Magic service stopped unexpectedly ({}).\n\n\
                 Automatic restarts failed {} times, so it was not brought back.\n\
                 Please restart the application and check the log file for the \
                 backend's last output.",
                exit.status, MAX_BACKEND_RESTARTS
            );

            log::error!(
                "[backend] giving up after {} restart attempts",
                MAX_BACKEND_RESTARTS
            );
            let _ = app_handle.emit("backend-error", serde_json::json!({ "message": msg }));
            show_error_dialog(&app_handle, "Go Magic Service Stopped", &msg);
            return;
        }

        let delay = restart_delay(failures);
        failures += 1;

        log::warn!(
            "[backend] restarting in {:.0}s (attempt {}/{})",
            delay.as_secs_f64(),
            failures,
            MAX_BACKEND_RESTARTS
        );

        if sleep_unless_shutting_down(delay) {
            return;
        }

        if restart_backend(&app_handle, &resource_dir) {
            failures = 0;
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
        log::error!("[backend] restart failed: resource directory is unavailable");
        return;
    };

    // A restart takes seconds; never block the IPC thread on it.
    let _ = app_handle.emit("backend-restarting", serde_json::json!({}));

    thread::spawn(move || {
        // Serialise with the supervisor. Without this, a manual restart landing
        // on the same tick as an automatic one spawns two backends, which then
        // fight over the port and over `sessions.db`.
        let _guard = match RESTART_GUARD.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        restart_backend(&app_handle, &resource_dir);
    });
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

            install_backend(process, port);

            // From here on the backend is watched for the app's whole lifetime:
            // a child that dies must be restarted instead of leaving the window
            // pointing at a closed port.
            {
                let app_handle = app_handle.clone();
                let resource_dir = resource_dir.clone();
                thread::spawn(move || supervise_backend(app_handle, resource_dir));
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

            // Whether the window has to come back maximized. The flag is applied
            // below, once the window exists — see `reveal_maximized`.
            let wants_maximize = window_state.maximized;

            let builder = WebviewWindowBuilder::new(
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
            // A window that is about to be maximized is created *hidden*, and on
            // Windows tao is never asked to maximize it: tao maximizes a hidden
            // window with `ShowWindow(SW_MAXIMIZE)` and then hides it again right
            // away, because its stored flags still say "hidden" (`apply_diff` ends
            // with `ShowWindow(SW_HIDE)` whenever VISIBLE is unset). The window
            // therefore shows up for an instant and vanishes — the "flashes and
            // then disappears" of a maximized restore. tao 0.37.0 fixed that
            // ordering (tao#1306), but every released tauri still pins tao
            // `^0.35`, so we reveal the window ourselves instead, in a single step
            // (`reveal_maximized` below). A plain window keeps the default
            // `visible: true` and appears immediately, exactly as before.
            .visible(!wants_maximize)
            // Devtools are enabled in debug builds, and in release builds only
            // when the `devtools` cargo feature is opted into explicitly.
            .devtools(cfg!(any(debug_assertions, feature = "devtools")))
            // Disable Tauri's native drag-drop handler. On Windows it installs
            // its own IDropTarget on the webview's HWND, which swallows every
            // drag before WebView2 can turn it into HTML5 events — so pages
            // never see `dragover`/`drop` and HTML5 drag & drop (kanban cards,
            // file-drop install zones) silently breaks in packaged builds while
            // working in a browser. The frontend only uses HTML5 DnD and never
            // the Tauri drag-drop API, so disabling the handler costs nothing.
            .disable_drag_drop_handler()
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
            });

            // tao is only handed the maximized flag where its own ordering is
            // safe. On Windows it is applied while the window is still hidden,
            // which is precisely the flash described above; there the flag is
            // applied by `reveal_maximized` after the window exists.
            #[cfg(not(windows))]
            let builder = builder.maximized(wants_maximize);

            let window = match builder.build() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create window: {}", e);
                    return Err(e.into());
                }
            };

            // Restore the maximized state that was saved with the window state.
            // Doing it here rather than through the builder also avoids the
            // "restore a monitor-sized plain window" trap: the window manager
            // recomputes the frame itself, so the window really does fill the
            // screen instead of landing a few pixels off.
            if wants_maximize {
                #[cfg(debug_assertions)]
                println!("Restoring maximized window");

                // Windows: reveal the window already maximized, in one step.
                #[cfg(windows)]
                let revealed = reveal_maximized(&window);
                #[cfg(not(windows))]
                let revealed = false;

                if !revealed {
                    // Fallback (and the path other platforms always take): let tao
                    // maximize the hidden window, then reveal it. Whatever the
                    // ordering costs on this platform, the first frame the user
                    // sees is the final, maximized one.
                    let _ = window.maximize();
                }

                // This is also what flips tao's internal VISIBLE flag. Without it
                // tao still believes the window is hidden, and the next flag
                // change would hide it for real (`apply_diff` ends with
                // `ShowWindow(SW_HIDE)` whenever VISIBLE is unset). On the Windows
                // path the window is already on screen and maximized, so this call
                // only syncs tao's bookkeeping.
                let _ = window.show();
            }

            #[cfg(any(debug_assertions, feature = "devtools"))]
            {
                window.open_devtools();
            }

            // Must come after `show()`: focus requests on a hidden window are
            // ignored, and a maximized window only exists after the reveal above.
            let _ = window.set_focus();

            // Wait for the backend off the UI thread, then make sure the window
            // is actually showing it. The window's very first load races the
            // backend and may briefly show a connection error; this reloads it
            // once the server answers.
            {
                let app_handle = app_handle.clone();
                let window = window.clone();

                thread::spawn(move || {
                    let generation = backend_generation();

                    if wait_for_backend_ready(port, Some(generation), &app_handle) {
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

                        return;
                    }

                    // Two different failures share this branch. If the child
                    // died during startup — or a supervised restart already
                    // replaced it — recovery belongs to the supervisor, and
                    // quitting here would kill the app just as the backend
                    // comes back.
                    if backend_generation() != generation || !backend_child_running() {
                        log::warn!(
                            "[backend] no answer within {}s; leaving recovery to the supervisor",
                            HEALTH_CHECK_TIMEOUT.as_secs()
                        );
                        return;
                    }

                    // Still alive but never answered: nothing has been loaded
                    // yet, so fail loudly rather than leave an empty window.
                    shutdown_backend();

                    let msg = format!(
                        "Backend did not become ready within {}s.\nPlease check the log file for details.",
                        HEALTH_CHECK_TIMEOUT.as_secs()
                    );
                    log::error!("[backend] {}", msg);
                    let _ = app_handle
                        .emit("backend-error", serde_json::json!({ "message": msg }));
                    show_error_dialog(&app_handle, "Application Startup Failed", &msg);
                    app_handle.exit(1);
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
                    // Sets the shutdown flag first, so the supervisor cannot
                    // interpret the intentional kill as a crash and restart the
                    // backend while the app is exiting.
                    shutdown_backend();
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

// Tests live in their own module: `tests.rs`.
#[cfg(test)]
mod tests;

