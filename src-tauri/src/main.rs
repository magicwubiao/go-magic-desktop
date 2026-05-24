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

// ============================================================================
// Window State Management
// ============================================================================

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct WindowState {
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            width: 1400.0,
            height: 900.0,
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

fn save_window_state(app_handle: &AppHandle) {
    let Some(path) = get_window_state_path(app_handle) else {
        return;
    };
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };

    let state = WindowState {
        width: match window.inner_size() {
            Ok(size) => size.width as f64,
            Err(_) => 1400.0,
        },
        height: match window.inner_size() {
            Ok(size) => size.height as f64,
            Err(_) => 900.0,
        },
        x: match window.inner_position() {
            Ok(pos) => Some(pos.x as f64),
            Err(_) => None,
        },
        y: match window.inner_position() {
            Ok(pos) => Some(pos.y as f64),
            Err(_) => None,
        },
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&PathBuf::from("")));
        let _ = std::fs::write(&path, json);
        #[cfg(debug_assertions)]
        println!("Saved window state to {:?}", path);
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
        Ok(content) => serde_json::from_str::<WindowState>(&content).unwrap_or_default(),
        Err(_) => WindowState::default(),
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

fn show_error_dialog(_app_handle: &AppHandle, title: &str, message: &str) {
    eprintln!("{}: {}", title, message);
}

#[cfg(not(target_os = "windows"))]
fn ensure_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        if (perms.mode() & 0o111) != 0o111 {
            perms.set_mode(perms.mode() | 0o755);
            if std::fs::set_permissions(path, perms).is_ok() {
                #[cfg(debug_assertions)]
                println!("Set executable permissions for: {:?}", path);
                return true;
            }
        } else {
            return true;
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn ensure_executable(_path: &Path) -> bool {
    true
}

fn find_backend_path(_resource_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let binary_names = vec!["go-magic.exe", "go-magic"];

    #[cfg(not(target_os = "windows"))]
    let binary_names = vec!["go-magic"];

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            for name in &binary_names {
                let path = exe_dir.join(name);
                if path.exists() {
                    if !ensure_executable(&path) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "Warning: Failed to set executable permissions for {:?}",
                            path
                        );
                    }
                    return Some(path);
                }
            }

            #[cfg(target_os = "macos")]
            {
                let resources_dir = exe_dir.join("../Resources");
                if resources_dir.exists() {
                    for name in &binary_names {
                        let path = resources_dir.join(name);
                        if path.exists() {
                            if !ensure_executable(&path) {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "Warning: Failed to set executable permissions for {:?}",
                                    path
                                );
                            }
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    let path_binary = "go-magic.exe";
    #[cfg(not(target_os = "windows"))]
    let path_binary = "go-magic";

    if Command::new(path_binary).arg("--version").output().is_ok() {
        return Some(PathBuf::from(path_binary));
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
                if let Ok(line) = line {
                    #[cfg(debug_assertions)]
                    println!("[backend] {}", line);
                }
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
                BackendError::HealthCheckTimeout => "Backend health check timed out".to_string(),
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
        "version": env!("CARGO_PKG_VERSION"),
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "mode": "process-isolated"
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
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
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
                    show_error_dialog(&app_handle, "启动错误", &format!("无法获取资源目录: {}", e));
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

                    let mut window_builder = WebviewWindowBuilder::new(
                        app,
                        "main",
                        WebviewUrl::External(server_url.parse().unwrap()),
                    )
                    .title("Go Magic")
                    .inner_size(window_state.width, window_state.height)
                    .min_inner_size(1000.0, 700.0)
                    .focused(true)
                    .resizable(true)
                    .fullscreen(false);

                    if let (Some(x), Some(y)) = (window_state.x, window_state.y) {
                        window_builder = window_builder.position(x, y);
                    } else {
                        window_builder = window_builder.center();
                    }

                    let window = window_builder.build().expect("Failed to create window");

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
                        BackendError::NoPortAvailable => "没有可用的端口".to_string(),
                        BackendError::BackendNotFound => {
                            format!("找不到后端可执行文件\n请检查资源目录: {:?}", resource_dir)
                        }
                        BackendError::SpawnFailed(msg) => format!("启动后端失败: {}", msg),
                        BackendError::HealthCheckTimeout => {
                            "后端健康检查超时，请检查后端是否正常工作".to_string()
                        }
                    };

                    eprintln!("Failed to start backend: {:?}", error_msg);
                    show_error_dialog(&app_handle, "应用启动失败", &error_msg);
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
                tauri::WindowEvent::Resized { .. } | tauri::WindowEvent::Moved { .. } => {
                    save_window_state(&app_handle);
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}
