//! Backend process management: spawn, supervise, restart, and stop the
//! bundled `go-magic` child, plus port picking and the raw-socket health
//! probe. Everything here is UI-agnostic and unit-testable; the Tauri glue
//! lives in `main.rs` and `update.rs`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, TryLockError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::locales::{fill, shell_text};

// --------------------------------------------------------------------------
// Constants
// --------------------------------------------------------------------------

/// Ports the backend tries first, in this order. Kept short and stable so the
/// documented default (`5000`) holds whenever it is free.
pub(crate) const DEFAULT_PORTS: &[u16] = &[5000, 5001, 5002];
/// Where the random fallback probing starts: the low end of the Windows
/// dynamic-port range (macOS starts at 49152 too; Linux's 32768..60999
/// contains it). Preferred over scanning a fixed low range — the old
/// `8000..9000` / `3000` / `8080` fallbacks sit exactly where dev servers and
/// proxies like to bind, so they both caused and collided with conflicts.
pub(crate) const EPHEMERAL_PORT_MIN: u16 = 49152;
/// How many random high ports to try before giving up.
const EPHEMERAL_PORT_ATTEMPTS: u32 = 64;
pub(crate) const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);
/// Timeout for a single health probe: connect, write and read alike.
pub(crate) const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Upper bound on how many bytes are read while hunting for the status line.
const MAX_STATUS_LINE_BYTES: usize = 1024;
/// How often the `backend-status` event is emitted while waiting for the backend.
pub(crate) const BACKEND_STATUS_INTERVAL: Duration = Duration::from_secs(1);
/// Grace period for the child process to die after being asked to terminate.
pub(crate) const BACKEND_STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the supervisor checks whether the backend is still running.
pub(crate) const SUPERVISE_INTERVAL: Duration = Duration::from_secs(1);
/// Consecutive automatic restart attempts before the supervisor gives up.
///
/// Reset as soon as the backend survives a full [`SUPERVISE_INTERVAL`], so a
/// one-off crash hours into a session does not consume the whole budget.
pub(crate) const MAX_BACKEND_RESTARTS: u32 = 5;
/// First backoff step; doubled on every further failed attempt.
pub(crate) const AUTO_RESTART_BASE_DELAY: Duration = Duration::from_secs(1);
/// Ceiling for the exponential backoff.
pub(crate) const AUTO_RESTART_MAX_DELAY: Duration = Duration::from_secs(30);
/// How many stderr lines are kept around to explain an unexpected exit.
pub(crate) const BACKEND_STDERR_TAIL_LINES: usize = 20;
/// How long `restart_backend` waits for the old port to actually be released
/// before spawning the replacement. Probing beats the old fixed `sleep(1s)`:
/// it returns as soon as the port is free, and only falls back to the full
/// budget when something is still holding it.
const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(1);
/// Poll interval for [`wait_for_port_release`].
const PORT_RELEASE_POLL: Duration = Duration::from_millis(50);

/// Set once the app is going away. The supervisor must never resurrect a
/// backend that was stopped on purpose.
pub(crate) static BACKEND_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
/// Serialises manual and automatic restarts: two concurrent restarts would
/// spawn two backends that then fight over the port and `sessions.db`.
pub(crate) static RESTART_GUARD: Mutex<()> = Mutex::new(());

// --------------------------------------------------------------------------
// Process Bookkeeping
// --------------------------------------------------------------------------

pub(crate) struct BackendState {
    process: Child,
    port: u16,
    start_time: Instant,
}

static BACKEND_STATE: Mutex<Option<BackendState>> = Mutex::new(None);
/// Incremented every time a new backend process is installed.
///
/// The startup health check uses it to tell "my backend never answered" from
/// "the supervisor already replaced it", which need opposite reactions.
pub(crate) static BACKEND_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Last stderr lines of the current child, kept so a crash can be explained.
///
/// The desktop log already carries everything via `tauri-plugin-log`, but a
/// panic is printed once and then drowned by request logging; repeating the
/// tail next to the exit status makes the cause impossible to miss.
static BACKEND_STDERR_TAIL: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Installs a freshly spawned backend, replacing whatever was registered before.
pub(crate) fn install_backend(process: Child, port: u16) {
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

pub(crate) fn backend_generation() -> u64 {
    BACKEND_GENERATION.load(Ordering::SeqCst)
}

/// The port of the currently registered backend, if one exists.
pub(crate) fn current_backend_port() -> Option<u16> {
    let guard = match BACKEND_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.as_ref().map(|state| state.port)
}

/// True while the registered backend process is still running.
///
/// `Err` counts as running: an unreadable child is not evidence of a crash, and
/// guessing "dead" here would make the app quit on a transient OS error.
pub(crate) fn backend_child_running() -> bool {
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
pub(crate) fn describe_exit_status(status: ExitStatus) -> String {
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
pub(crate) fn restart_delay(attempt: u32) -> Duration {
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

pub(crate) fn remember_stderr_line(line: &str) {
    let mut tail = BACKEND_STDERR_TAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if tail.len() >= BACKEND_STDERR_TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line.to_string());
}

pub(crate) fn clear_stderr_tail() {
    BACKEND_STDERR_TAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

pub(crate) fn backend_stderr_tail() -> Vec<String> {
    BACKEND_STDERR_TAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// What the supervisor knows about a backend that is no longer there.
pub(crate) struct BackendExit {
    pub(crate) status: String,
    pub(crate) uptime: Duration,
    pub(crate) port: u16,
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

/// Upper bound on a single loopback connect probe. This is only a latency
/// cap, NOT a "busy" signal: a free port is *not* guaranteed to refuse the
/// connect instantly. Firewalls, IPv6-disabled hosts and CI runners can drop
/// the SYN/RST so the probe times out on a completely idle port. Treating a
/// timeout as "busy" made every free port read as occupied, so
/// `pick_available_port` fell through all candidates and the port regression
/// tests failed right after their holder was dropped. Only an established
/// connection proves a live listener; refusal, timeout and everything else
/// are "no listener observed" and fall through to the next probe.
const PORT_PROBE_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);

/// Probes the port the same way the backend will bind it, plus the one way
/// Windows can lie about it.
///
/// The Go backend listens on the wildcard (`:%d`), so the probe must claim
/// the wildcard on *both* families — a failing bind is a hard "busy".
///
/// The inverse is NOT true on Windows: a *succeeding* bind does not prove
/// the port is free. Without `SO_EXCLUSIVEADDRUSE` (which neither Rust's
/// std nor Go sets), Windows happily lets a second `bind()` against an
/// in-use wildcard port succeed. Verified against a leftover backend that
/// held `:5000`: the probe bind succeeded, the real child then died with
/// `WSAEADDRINUSE`, and because the leftover answered the `/health` probe
/// the supervisor treated every restart as a success — reloading the
/// window every two seconds, forever. A loopback `connect()` cannot be
/// fooled this way: a live listener on the port accepts the connection, and
/// that success is the only reliable proof of a holder. A refusal OR a
/// timeout means "no listener answered" — a free port can time out too when
/// SYN/RST packets are filtered, so a timeout must NOT be read as busy.
/// Both families are probed so an IPv6-only holder is caught as well.
pub(crate) fn is_port_available(port: u16) -> bool {
    if TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_err() {
        return false;
    }

    // The dual-stack bind probe only matters on Windows, where the duplicate-
    // bind quirk below lets a stale AF_INET6 holder slip past the AF_INET
    // probe. On Unix a duplicate bind fails strictly, and skipping the probe
    // there keeps IPv6-less kernels (some minimal Linux installs) from
    // reporting every port as busy.
    #[cfg(target_os = "windows")]
    if TcpListener::bind((Ipv6Addr::UNSPECIFIED, port)).is_err() {
        return false;
    }

    for addr in [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
    ] {
        // An established connection is the only proof a live listener holds
        // the port — including the Windows bind-lie case above. Everything
        // else (ConnectionRefused, TimedOut, network-unreachable, ...) means
        // "no listener observed" and must fall through. A free port is NOT
        // guaranteed to refuse quickly: filtered SYN/RST (firewalls,
        // IPv6-disabled hosts, CI runners) make an idle port time out too, so
        // reading a timeout as busy would mark every free port occupied and
        // starve `pick_available_port` of all candidates.
        if TcpStream::connect_timeout(&addr, PORT_PROBE_CONNECT_TIMEOUT).is_ok() {
            return false;
        }
    }

    true
}

pub(crate) fn pick_available_port() -> Option<u16> {
    for &port in DEFAULT_PORTS {
        if is_port_available(port) {
            return Some(port);
        }
    }

    // Randomised high ports instead of a fixed scan: two app instances (or an
    // app and anything else that scans the same fixed range) picking the same
    // predictable candidate is exactly the race that starts crash loops.
    let mut rng = port_rng_seed();
    let range = (u16::MAX - EPHEMERAL_PORT_MIN) as u64 + 1;
    for _ in 0..EPHEMERAL_PORT_ATTEMPTS {
        let port = EPHEMERAL_PORT_MIN + (rng.next() % range) as u16;
        if is_port_available(port) {
            return Some(port);
        }
    }
    None
}

/// Seed for the port probe's tiny xorshift generator: time and pid mixed with
/// splitmix64 constants. `std` only — no rand dependency for one number.
pub(crate) fn port_rng_seed() -> XorShift64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = u64::from(std::process::id());
    let mut seed = nanos ^ pid.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    // xorshift must never start from zero.
    if seed == 0 {
        seed = 0xA076_1D64_78BD_642F;
    }
    XorShift64(seed)
}

/// Minimal xorshift64* generator for the port probe. Not cryptographic — it
/// only needs to scatter candidates over the ephemeral range.
pub(crate) struct XorShift64(pub(crate) u64);

impl XorShift64 {
    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// Waits up to `budget` for `port` to become bindable again.
///
/// Used before spawning a replacement backend: a fresh child that picks the
/// same port would die on a bind that has not been released yet. Probing
/// returns early the moment the port is free instead of always paying the
/// fixed wait.
fn wait_for_port_release(port: Option<u16>, budget: Duration) {
    let Some(port) = port else {
        return;
    };
    if port == 0 {
        return;
    }

    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if is_port_available(port) {
            return;
        }
        thread::sleep(PORT_RELEASE_POLL);
    }

    // Something outside our control still holds the port. The child will try
    // its own bind (Go may set SO_REUSEADDR, and a replacement may simply
    // pick another port), so this is a warning, not an error.
    log::warn!(
        "[backend] port {} still busy after {}ms; spawning the replacement anyway",
        port,
        budget.as_millis()
    );
}

// --------------------------------------------------------------------------
// Health Check
// --------------------------------------------------------------------------

/// Extracts the status code from an HTTP/1.x status line (`HTTP/1.1 200 OK`).
///
/// Anything that is not a status line — a TLS alert, a stray banner, an empty
/// response — yields `None`, which the caller treats as "not healthy".
pub(crate) fn parse_status_code(status_line: &str) -> Option<u16> {
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
pub(crate) fn check_backend_health(port: u16) -> bool {
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
pub(crate) fn wait_for_backend_ready(
    port: u16,
    generation: Option<u64>,
    app_handle: &AppHandle,
) -> bool {
    use tauri::Emitter;

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
                println!(
                    "Health probe answered but the backend child is gone; treating as not ready"
                );

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
// Spawn
// --------------------------------------------------------------------------

/// Spawns the bundled backend **without** waiting for it to become healthy.
///
/// Keeping the spawn free of any blocking work is what lets `setup` create the
/// window immediately; the health check runs on a background thread instead.
pub(crate) fn spawn_backend(resource_dir: &std::path::Path) -> Result<(Child, u16), BackendError> {
    let port = pick_available_port().ok_or(BackendError::NoPortAvailable)?;

    #[cfg(debug_assertions)]
    println!("Selected port: {}", port);

    let backend_path =
        crate::find_backend_path(resource_dir).ok_or(BackendError::BackendNotFound)?;

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

    // Own process group on Unix: lets `stop_backend` signal the whole tree
    // (the backend may spawn helpers of its own), mirroring what
    // `taskkill /T /F` does on Windows.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    // A Rust backtrace is useful while developing and noise in a shipped build
    // (it would only leak internal paths into the log file).
    #[cfg(debug_assertions)]
    command.env("RUST_BACKTRACE", "1");

    // Don't flash a console window on Windows.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
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

// --------------------------------------------------------------------------
// Process Control
// --------------------------------------------------------------------------

/// Terminates the backend: force-kill the whole tree immediately, then reap.
///
/// An earlier version tried a "graceful" console signal first
/// (`AttachConsole` + `GenerateConsoleCtrlEvent(CTRL_BREAK)`), but that whole
/// path was dead code: a console child spawned with `CREATE_NO_WINDOW` never
/// gets a console at all ("the console handle for the application is not set"),
/// so `AttachConsole` failed with `ERROR_INVALID_HANDLE` on every attempt and
/// execution always fell through to the force kill below. The console FFI only
/// added risk and complexity, so it is gone.
///
/// `taskkill /T /F` takes down the backend together with any helper processes
/// it spawned. The Go backend loses its chance to flush on `SIGTERM`-like
/// signals, but SQLite-based state survives a hard kill the same way it always
/// has in practice — this was already the effective behavior.
pub(crate) fn stop_backend() {
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

    // On Windows the Go backend may have helper processes of its own; kill
    // the whole tree so nothing is left running behind the app's back.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = Command::new("taskkill")
            .args(["/PID", &state.process.id().to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    // Unix: kill the whole process group (`spawn_backend` created one with
    // `process_group(0)`), then the child itself.
    #[cfg(unix)]
    {
        let pid = state.process.id() as libc::pid_t;
        unsafe { libc::kill(-pid, libc::SIGKILL) };
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

/// Stops the backend because the app is going away, then exits the app.
///
/// The whole thing runs on a background thread: `stop_backend` can block for
/// up to 2×[`BACKEND_STOP_TIMEOUT`] (grace period + zombie reaping), and doing
/// that on the main thread froze the UI for the entire quit. The flag is set
/// *before* the thread spawns so the supervisor can never interpret the
/// intentional kill as a crash.
pub(crate) fn shutdown_and_exit(app_handle: &AppHandle) {
    BACKEND_SHUTTING_DOWN.store(true, Ordering::SeqCst);

    let app_handle = app_handle.clone();
    thread::spawn(move || {
        stop_backend();
        // Safe from any thread: exit posts to the main event loop, which is
        // idle by now (the window is already closing / menu was handled).
        app_handle.exit(0);
    });
}

/// Restarts the backend and waits for the replacement to answer.
///
/// Returns `true` when a healthy replacement is running. Blocks for up to
/// [`HEALTH_CHECK_TIMEOUT`], so call it from a background thread (see
/// [`crate::restart_backend_cmd`] and [`supervise_backend`]).
pub(crate) fn restart_backend(app_handle: &AppHandle, resource_dir: &std::path::Path) -> bool {
    use tauri::Emitter;

    #[cfg(debug_assertions)]
    println!("Restarting backend...");

    log::info!("[backend] restart requested");

    // The old port is only known while the old child is still registered.
    let old_port = current_backend_port();

    stop_backend();

    // A restart queued just before the window closed must not spawn a process
    // that then outlives the app.
    if BACKEND_SHUTTING_DOWN.load(Ordering::SeqCst) {
        return false;
    }

    // Give the OS time to release the old port, but return early the moment
    // it is free — the old fixed one-second sleep paid the full price every
    // time, healthy port release or not.
    wait_for_port_release(old_port, PORT_RELEASE_TIMEOUT);

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
        let msg = fill(
            shell_text().dialog_backend_not_ready_restart,
            &[&HEALTH_CHECK_TIMEOUT.as_secs().to_string()],
        );
        log::error!("[backend] {}", msg);
        let _ = app_handle.emit("backend-error", serde_json::json!({ "message": msg }));

        // A replacement that never answers is no better than a dead one. Drop
        // it so the supervisor sees "no backend" on its next tick and retries
        // with backoff, instead of leaving a half-started process behind.
        stop_backend();
        return false;
    }

    // The backend is healthy now: record completion BEFORE touching the
    // window, so the log always shows the restart outcome. (An earlier
    // version logged only after the reload — the log went silent whenever
    // the webview dispatch panicked, and the restart looked like "nothing
    // happened".)
    log::info!("[backend] restarted on port {}", port);

    #[cfg(debug_assertions)]
    println!("Backend restarted on port {}", port);

    // The new instance may have picked a different port, so point the window
    // at it again. Strictly best-effort, wrapped in an unwind guard: a panic
    // in the webview dispatch (poisoned runtime locks, destroyed webview)
    // used to kill this whole background thread with nothing in any log —
    // the backend was already fine, only the page failed to follow. Now the
    // panic is recorded and the restart still counts as successful; on the
    // usual same-port restart the user simply refreshes the page (F5).
    let reload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Ok(url) = format!("http://127.0.0.1:{}/", port).parse::<tauri::Url>() {
                let _ = window.navigate(url);
                return;
            }
        }
        log::warn!("[backend] no main window to reload after restart");
    }));
    if let Err(panic) = reload {
        log::error!(
            "[backend] window reload after restart panicked: {}",
            panic_message(&panic)
        );
    }

    let _ = app_handle.emit("backend-restarted", port);

    true
}

/// Best-effort panic payload extraction for logging: panics carry either a
/// `&'static str` or a `String`; anything else falls back to a generic line.
fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
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
pub(crate) fn supervise_backend(app_handle: AppHandle, resource_dir: std::path::PathBuf) {
    use tauri::Emitter;

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
            let msg = fill(
                shell_text().dialog_service_stopped,
                &[&exit.status.to_string(), &MAX_BACKEND_RESTARTS.to_string()],
            );

            log::error!(
                "[backend] giving up after {} restart attempts",
                MAX_BACKEND_RESTARTS
            );
            let _ = app_handle.emit("backend-error", serde_json::json!({ "message": msg }));
            crate::show_error_dialog(&app_handle, shell_text().dialog_service_stopped_title, &msg);
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

        // Deliberately NOT resetting `failures` after a successful restart.
        // A restart "success" only means the health probe answered while the
        // child was alive; a port quirk (or a foreign server on the port) can
        // make every attempt "succeed" while the child keeps dying seconds
        // later, which produced an endless attempt-1/5 reload loop. The budget
        // is refunded in the branch above, once the backend has actually
        // survived a full supervise interval.
        restart_backend(&app_handle, &resource_dir);
    }
}

// --------------------------------------------------------------------------
// Errors
// --------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum BackendError {
    NoPortAvailable,
    BackendNotFound,
    SpawnFailed(String),
    /// Constructed only from tests right now (they verify the localized
    /// Display output); suppressed in non-test builds where it is dead.
    #[cfg_attr(not(test), expect(dead_code))]
    HealthCheckTimeout,
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // These strings reach the startup error dialog and logs, so they come
        // from the shell text tables (see `locales.rs`) rather than being
        // hardcoded English. The dialogs for BackendNotFound build richer
        // messages in `setup`, so this short form is a fallback.
        let text = shell_text();
        match self {
            BackendError::NoPortAvailable => write!(f, "{}", text.error_no_port),
            BackendError::BackendNotFound => write!(f, "{}", text.error_backend_not_found),
            BackendError::SpawnFailed(msg) => {
                write!(f, "{}", fill(text.error_spawn_failed, &[msg.as_str()]))
            }
            BackendError::HealthCheckTimeout => write!(f, "{}", text.error_health_check_timeout),
        }
    }
}

impl std::error::Error for BackendError {}
