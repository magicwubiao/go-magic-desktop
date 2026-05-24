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
use tauri_plugin_dialog::{Dialog, MessageDialogButtons, MessageDialogKind};

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
    let _ = Dialog::message(message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::Ok)
        .blocking_show(app_handle);
}

#[cfg(not(target_os = "windows"))]
fn ensure_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        if !perms.mode() & 0o111 != 0 {
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
    true // Windows doesn't use executable bits like Unix
}

fn find_backend_path(resource_dir: &Path) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    println!("Searching for backend in resource dir: {:?}", resource_dir);
    
    // 列出资源目录的内容，帮助调试
    #[cfg(debug_assertions)]
    {
        if let Ok(entries) = std::fs::read_dir(resource_dir) {
            println!("Resource dir contents:");
            for entry in entries.flatten() {
                println!("  - {:?}", entry.path());
            }
        }
    }

    #[cfg(target_os = "windows")]
    let names = vec![
        "go-magic.exe",
        "resources/go-magic.exe",
        "bin/go-magic.exe",
        "backends/go-magic.exe",
    ];

    #[cfg(not(target_os = "windows"))]
    let names = vec![
        "go-magic",
        "resources/go-magic",
        "bin/go-magic",
        "backends/go-magic",
    ];

    // 1. 先在资源目录中查找（Tauri 标准打包位置）
    for name in &names {
        let path = resource_dir.join(name);
        #[cfg(debug_assertions)]
        println!("Checking in resource dir: {:?}", path);
        if path.exists() {
            #[cfg(debug_assertions)]
            println!("Found backend at: {:?}", path);
            
            // Ensure executable permissions
            if !ensure_executable(&path) {
                #[cfg(debug_assertions)]
                eprintln!("Warning: Failed to set executable permissions for {:?}", path);
            }
            
            return Some(path);
        }
    }

    // 2. 如果在资源目录没找到，尝试在可执行文件同级目录查找（某些打包方式）
    if let Ok(exe_dir) = std::env::current_exe().and_then(|p| {
        p.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No parent dir"))
    }) {
        #[cfg(debug_assertions)]
        println!("Searching in exe dir: {:?}", exe_dir);
        
        for name in &names {
            let path = exe_dir.join(name);
            #[cfg(debug_assertions)]
            println!("Checking in exe dir: {:?}", path);
            if path.exists() {
                #[cfg(debug_assertions)]
                println!("Found backend in exe dir: {:?}", path);
                
                if !ensure_executable(&path) {
                    #[cfg(debug_assertions)]
                    eprintln!("Warning: Failed to set executable permissions for {:?}", path);
                }
                
                return Some(path);
            }
        }
        
        // 2.1 macOS App Bundle 特殊处理：尝试在 ../Resources 查找
        #[cfg(target_os = "macos")]
        {
            let resources_dir = exe_dir.join("../Resources");
            if resources_dir.exists() {
                #[cfg(debug_assertions)]
                println!("Searching in macOS Resources dir: {:?}", resources_dir);
                
                for name in &names {
                    let path = resources_dir.join(name);
                    if path.exists() {
                        #[cfg(debug_assertions)]
                        println!("Found backend in macOS Resources: {:?}", path);
                        
                        if !ensure_executable(&path) {
                            #[cfg(debug_assertions)]
                            eprintln!("Warning: Failed to set executable permissions for {:?}", path);
                        }
                        
                        return Some(path);
                    }
                }
            }
        }
    }

    // 3. 最后尝试 PATH
    #[cfg(target_os = "windows")]
    let binary = "go-magic.exe";
    #[cfg(not(target_os = "windows"))]
    let binary = "go-magic";

    if Command::new(binary).arg("--version").output().is_ok() {
        #[cfg(debug_assertions)]
        println!("Using backend from PATH: {}", binary);
        return Some(PathBuf::from(binary));
    }

    None
}

enum BackendError {
    NoPortAvailable,
    BackendNotFound,
    SpawnFailed(String),
    HealthCheckTimeout,
}

fn start_backend(app_handle: &AppHandle, resource_dir: &Path) -> Result<(Child, u16), BackendError> {
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

    // Backend output capture thread
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
                    show_error_dialog(
                        &app_handle,
                        "启动错误",
                        &format!("无法获取资源目录: {}", e),
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

                    let window = WebviewWindowBuilder::new(
                        app,
                        "main",
                        WebviewUrl::External(server_url.parse().unwrap()),
                    )
                    .title("Go Magic")
                    .inner_size(1400.0, 900.0)
                    .min_inner_size(1000.0, 700.0)
                    .center()
                    .focused(true)
                    .resizable(true)
                    .fullscreen(false)
                    .build()
                    .expect("Failed to create window");

                    // Open DevTools in debug mode
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
                        BackendError::BackendNotFound => format!("找不到后端可执行文件\n请检查资源目录: {:?}", resource_dir),
                        BackendError::SpawnFailed(msg) => format!("启动后端失败: {}", msg),
                        BackendError::HealthCheckTimeout => "后端健康检查超时，请检查后端是否正常工作".to_string(),
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
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                stop_backend();
                window.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}