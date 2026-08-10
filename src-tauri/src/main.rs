#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Go Magic Desktop - Process Isolation Mode

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri::webview::NewWindowResponse;
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

        return (new_x, new_y);
    }
    (x, y)
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
            let logical = size.to_logical(scale_factor);
            (logical.width as f64, logical.height as f64)
        }
        Err(_) => (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT),
    };

    let (x, y) = match window.outer_position() {
        Ok(pos) => {
            let logical = pos.to_logical(scale_factor);
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
            Ok(mut state) => {
                if state.width < MIN_WINDOW_WIDTH {
                    state.width = MIN_WINDOW_WIDTH;
                }
                if state.height < MIN_WINDOW_HEIGHT {
                    state.height = MIN_WINDOW_HEIGHT;
                }
                state
            }
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
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 60;
const HEALTH_CHECK_INTERVAL_MS: u64 = 500;

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

fn check_backend_health(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok();

    client
        .and_then(|c| c.get(&url).send().ok().map(|r| r.status().is_success()))
        .unwrap_or(false)
}

fn wait_for_backend_ready(port: u16, app_handle: &AppHandle) -> bool {
    let start = Instant::now();

    #[cfg(debug_assertions)]
    println!("Waiting for backend on port {}...", port);

    while start.elapsed().as_secs() < HEALTH_CHECK_TIMEOUT_SECS {
        let _ = app_handle.emit(
            "backend-status",
            serde_json::json!({
                "state": "starting",
                "elapsed": start.elapsed().as_secs(),
                "port": port
            }),
        );

        if check_backend_health(port) {
            #[cfg(debug_assertions)]
            println!("Backend ready after {}ms", start.elapsed().as_millis());
            return true;
        }

        thread::sleep(Duration::from_millis(HEALTH_CHECK_INTERVAL_MS));
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
    #[cfg(debug_assertions)]
    println!("Opening external link: {}", url);

    if let Err(e) = open::that(url) {
        eprintln!("Failed to open external link {}: {}", url, e);
    }
}

fn find_backend_path(resource_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let binary_names = vec!["go-magic.exe", "go-magic"];

    #[cfg(not(target_os = "windows"))]
    let binary_names = vec!["go-magic"];

    let mut search_paths = Vec::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // 1. Check exe directory
            for name in &binary_names {
                let path = exe_dir.join(name);
                search_paths.push(path.clone());
                if path.exists() {
                    #[cfg(debug_assertions)]
                    println!("Found backend in exe dir: {:?}", path);
                    return Some(path);
                }
            }

            // 2. Check resources directory relative to exe
            let resources_relative = exe_dir.join("resources");
            search_paths.push(resources_relative.clone());
            if resources_relative.exists() {
                for name in &binary_names {
                    let path = resources_relative.join(name);
                    search_paths.push(path.clone());
                    if path.exists() {
                        #[cfg(debug_assertions)]
                        println!("Found backend in exe dir resources: {:?}", path);
                        return Some(path);
                    }
                }
            }

            // 3. Check app directory (Windows: same level as resources)
            let app_dir = exe_dir.parent();
            if let Some(app_dir) = app_dir {
                for name in &binary_names {
                    let path = app_dir.join(name);
                    search_paths.push(path.clone());
                    if path.exists() {
                        #[cfg(debug_assertions)]
                        println!("Found backend in app dir: {:?}", path);
                        return Some(path);
                    }

                    // Check resources in app directory
                    let resources_in_app = app_dir.join("resources");
                    search_paths.push(resources_in_app.clone());
                    if resources_in_app.exists() {
                        let path = resources_in_app.join(name);
                        search_paths.push(path.clone());
                        if path.exists() {
                            #[cfg(debug_assertions)]
                            println!("Found backend in app resources: {:?}", path);
                            return Some(path);
                        }
                    }
                }
            }

            #[cfg(target_os = "macos")]
            {
                let resources_dir = exe_dir.join("../Resources");
                search_paths.push(resources_dir.clone());
                if resources_dir.exists() {
                    for name in &binary_names {
                        let path = resources_dir.join(name);
                        search_paths.push(path.clone());
                        if path.exists() {
                            #[cfg(debug_assertions)]
                            println!("Found backend in macOS Resources: {:?}", path);
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    // 4. Check Tauri resource_dir
    search_paths.push(resource_dir.to_path_buf());
    for name in &binary_names {
        let path = resource_dir.join(name);
        search_paths.push(path.clone());
        #[cfg(debug_assertions)]
        println!("Checking resource_dir: {:?} for {}", path, name);
        if path.exists() {
            #[cfg(debug_assertions)]
            println!("Found backend in resource_dir: {:?}", path);
            return Some(path);
        }
    }

    // 5. Check system PATH
    #[cfg(target_os = "windows")]
    let path_binary = "go-magic.exe";
    #[cfg(not(target_os = "windows"))]
    let path_binary = "go-magic";

    if Command::new(path_binary).arg("--version").output().is_ok() {
        #[cfg(debug_assertions)]
        println!("Found backend in PATH: {}", path_binary);
        return Some(PathBuf::from(path_binary));
    }

    // Log all searched paths for debugging
    eprintln!("Backend executable not found. Searched paths:");
    for (i, path) in search_paths.iter().enumerate() {
        eprintln!("  {}: {:?} (exists: {})", i + 1, path, path.exists());
    }

    None
}

enum BackendError {
    NoPortAvailable,
    BackendNotFound,
    SpawnFailed(String),
    HealthCheckTimeout,
}

fn start_backend(
    app_handle: &AppHandle,
    resource_dir: &Path,
) -> Result<(Child, u16), BackendError> {
    let port = pick_available_port().ok_or(BackendError::NoPortAvailable)?;

    #[cfg(debug_assertions)]
    println!("Selected port: {}", port);

    let backend_path = find_backend_path(resource_dir).ok_or(BackendError::BackendNotFound)?;

    #[cfg(debug_assertions)]
    println!("Backend path: {:?}", backend_path);

    #[cfg(target_os = "windows")]
    let mut child = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        Command::new(&backend_path)
            .creation_flags(CREATE_NO_WINDOW)
            .args(["server", "--port", &port.to_string()])
            .env("GOMAGIC_PORT", port.to_string())
            .env("RUST_BACKTRACE", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BackendError::SpawnFailed(e.to_string()))?
    };

    #[cfg(not(target_os = "windows"))]
    let mut child = {
        Command::new(&backend_path)
            .args(["server", "--port", &port.to_string()])
            .current_dir(resource_dir)
            .env("GOMAGIC_PORT", port.to_string())
            .env("RUST_BACKTRACE", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BackendError::SpawnFailed(e.to_string()))?
    };

    #[cfg(debug_assertions)]
    println!("Backend process spawned, PID: {:?}", child.id());

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    thread::spawn(move || {
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            for (i, line) in reader.lines().enumerate() {
                if i >= 50 {
                    break;
                }
                #[cfg(debug_assertions)]
                if let Ok(line) = line {
                    println!("[backend] {}", line);
                }
                #[cfg(not(debug_assertions))]
                let _ = line;
            }
        }
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            for (i, line) in reader.lines().enumerate() {
                if i >= 50 {
                    break;
                }
                if let Ok(line) = line {
                    if !line.is_empty() {
                        #[cfg(debug_assertions)]
                        eprintln!("[backend:err] {}", line);
                    }
                }
            }
        }
    });

    if wait_for_backend_ready(port, app_handle) {
        #[cfg(debug_assertions)]
        println!("Backend started successfully");
        Ok((child, port))
    } else {
        eprintln!("Backend failed to start within timeout");
        let _ = child.kill();
        Err(BackendError::HealthCheckTimeout)
    }
}

fn stop_backend() {
    if let Ok(mut guard) = BACKEND_STATE.lock() {
        if let Some(mut state) = guard.take() {
            let _runtime = state.start_time.elapsed().as_secs_f64();

            #[cfg(debug_assertions)]
            println!("Stopping backend (ran for {:.1}s)...", _runtime);

            match state.process.kill() {
                Ok(_) => {
                    #[cfg(debug_assertions)]
                    println!("Backend stopped");
                }
                Err(e) => eprintln!("Failed to stop backend: {}", e),
            }
        }
    }
}

fn restart_backend(app_handle: &AppHandle, resource_dir: &Path) {
    #[cfg(debug_assertions)]
    println!("Restarting backend...");
    stop_backend();
    thread::sleep(Duration::from_secs(1));

    match start_backend(app_handle, resource_dir) {
        Ok((process, port)) => {
            if let Ok(mut guard) = BACKEND_STATE.lock() {
                *guard = Some(BackendState {
                    process,
                    port,
                    start_time: Instant::now(),
                });
            }
            let _ = app_handle.emit("backend-restarted", port);
        }
        Err(e) => {
            let error_msg = match e {
                BackendError::NoPortAvailable => "No available port found".to_string(),
                BackendError::BackendNotFound => "Backend executable not found".to_string(),
                BackendError::SpawnFailed(msg) => format!("Failed to start backend: {}", msg),
                BackendError::HealthCheckTimeout => {
                    "Backend health check timed out, please verify if backend is working correctly"
                        .to_string()
                }
            };
            eprintln!("Restart failed: {}", error_msg);
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
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        restart_backend(&app_handle, &resource_dir);
    }
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
        "Starting {} v{} (commit: {}, built: {}, profile: {})",
        env!("CARGO_PKG_NAME"),
        env!("APP_VERSION"),
        option_env!("GIT_COMMIT").unwrap_or("unknown"),
        option_env!("BUILD_TIME").unwrap_or("unknown"),
        option_env!("BUILD_PROFILE").unwrap_or("unknown"),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .setup(|app| {
            let resource_dir = match app.path().resource_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    let app_handle = app.handle().clone();
                    show_error_dialog(
                        &app_handle,
                        "Startup Error",
                        &format!("Failed to get resource directory: {}", e),
                    );
                    return Err(e.into());
                }
            };
            let app_handle = app.handle().clone();

            #[cfg(debug_assertions)]
            println!("Resource directory: {:?}", resource_dir);

            match start_backend(&app_handle, &resource_dir) {
                Ok((process, port)) => {
                    {
                        let mut guard = BACKEND_STATE.lock().unwrap();
                        *guard = Some(BackendState {
                            process,
                            port,
                            start_time: Instant::now(),
                        });
                    }

                    let server_url = format!("http://127.0.0.1:{}/", port);

                    #[cfg(debug_assertions)]
                    println!("Creating window, URL: {}", server_url);

                    thread::sleep(Duration::from_millis(300));

                    let window_state = load_window_state(&app_handle);

                    #[cfg(debug_assertions)]
                    println!("Loaded window state: {:?}", window_state);

                    let (x, y) = match (window_state.x, window_state.y) {
                        (Some(x), Some(y)) => {
                            adjust_position_for_screen(
                                x,
                                y,
                                window_state.width,
                                window_state.height,
                                &app_handle,
                            )
                        }
                        _ => {
                            // 基于屏幕尺寸居中
                            if let Some(monitor) = app_handle.primary_monitor().ok().flatten() {
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
                            } else {
                                (0.0, 0.0)
                            }
                        }
                    };

                    // Intercept external links and open in system browser
                    let window = match WebviewWindowBuilder::new(
                        app,
                        "main",
                        WebviewUrl::External(server_url.parse().unwrap()),
                    )
                    .title("Go Magic")
                    .inner_size(window_state.width, window_state.height)
                    .min_inner_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
                    .position(x, y)
                    .focused(true)
                    .resizable(true)
                    .fullscreen(false)
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
                    .build() {
                        Ok(w) => w,
                        Err(e) => {
                            eprintln!("Failed to create window: {}", e);
                            return Err(e.into());
                        }
                    };

                    #[cfg(debug_assertions)]
                    {
                        window.open_devtools();
                    }

                    let _ = window.set_focus();

                    let _ = app_handle.emit(
                        "app-ready",
                        serde_json::json!({
                            "port": port,
                            "url": format!("http://127.0.0.1:{}", port)
                        }),
                    );

                    #[cfg(debug_assertions)]
                    println!("Application ready");
                }
                Err(e) => {
                    let error_msg = match &e {
                        BackendError::NoPortAvailable => "No available port found".to_string(),
                        BackendError::BackendNotFound => {
                            format!(
                                "Backend executable not found\nPlease check resource directory: {:?}",
                                resource_dir
                            )
                        }
                        BackendError::SpawnFailed(msg) => {
                            format!("Failed to start backend: {}", msg)
                        }
                        BackendError::HealthCheckTimeout => {
                            "Backend health check timed out, please verify if backend is working correctly".to_string()
                        }
                    };

                    eprintln!("Failed to start backend: {:?}", error_msg);
                    show_error_dialog(&app_handle, "Application Startup Failed", &error_msg);
                    return Err("Backend startup failed".into());
                }
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