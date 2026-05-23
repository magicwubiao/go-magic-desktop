#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Go Magic Desktop - 进程分离模式

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

// ============================================================================
// 常量配置
// ============================================================================

const DEFAULT_PORTS: &[u16] = &[5000, 5001, 5002, 5003, 5004, 8080, 3000];
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 60;
const HEALTH_CHECK_INTERVAL_MS: u64 = 500;

// ============================================================================
// 后端进程管理
// ============================================================================

struct BackendState {
    process: Child,
    port: u16,
    start_time: Instant,
}

static BACKEND_STATE: Mutex<Option<BackendState>> = Mutex::new(None);

// --------------------------------------------------------------------------
// 端口管理
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
    // 兜底：尝试 8000-9000 范围
    (8000..9000).find(|&port| is_port_available(port))
}

// --------------------------------------------------------------------------
// 健康检查
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
            println!("Backend ready after {}ms", start.elapsed().as_millis());
            return true;
        }

        thread::sleep(Duration::from_millis(HEALTH_CHECK_INTERVAL_MS));
    }

    false
}

// --------------------------------------------------------------------------
// 进程控制
// --------------------------------------------------------------------------

fn find_backend_path(resource_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let names = vec!["go-magic.exe", "bin/go-magic.exe", "backends/go-magic.exe"];

    #[cfg(not(target_os = "windows"))]
    let names = vec!["go-magic", "bin/go-magic", "backends/go-magic"];

    for name in &names {
        let path = resource_dir.join(name);
        if path.exists() {
            println!("Found backend at: {:?}", path);
            return Some(path);
        }
    }

    #[cfg(target_os = "windows")]
    let binary = "go-magic.exe";
    #[cfg(not(target_os = "windows"))]
    let binary = "go-magic";

    if Command::new(binary).arg("--version").output().is_ok() {
        println!("Using backend from PATH: {}", binary);
        return Some(PathBuf::from(binary));
    }

    None
}

fn start_backend(app_handle: &AppHandle, resource_dir: &Path) -> Option<(Child, u16)> {
    let port = pick_available_port()?;
    println!("Selected port: {}", port);

    let backend_path = find_backend_path(resource_dir)?;

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
            .ok()?
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
            .ok()?
    };

    println!("Backend process spawned, PID: {:?}", child.id());

    // 在后台线程中读取输出
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
                        eprintln!("[backend:err] {}", line);
                    }
                }
            }
        }
    });

    if wait_for_backend_ready(port, app_handle) {
        println!("Backend started successfully");
        Some((child, port))
    } else {
        eprintln!("Backend failed to start within timeout");
        let _ = child.kill();
        None
    }
}

fn stop_backend() {
    if let Ok(mut guard) = BACKEND_STATE.lock() {
        if let Some(mut state) = guard.take() {
            let runtime = state.start_time.elapsed().as_secs_f64();
            println!("Stopping backend (ran for {:.1}s)...", runtime);

            match state.process.kill() {
                Ok(_) => println!("Backend stopped"),
                Err(e) => eprintln!("Failed to stop backend: {}", e),
            }
        }
    }
}

fn restart_backend(app_handle: &AppHandle, resource_dir: &Path) {
    println!("Restarting backend...");
    stop_backend();
    thread::sleep(Duration::from_secs(1));

    if let Some((process, port)) = start_backend(app_handle, resource_dir) {
        if let Ok(mut guard) = BACKEND_STATE.lock() {
            *guard = Some(BackendState {
                process,
                port,
                start_time: Instant::now(),
            });
        }
        let _ = app_handle.emit("backend-restarted", port);
    }
}

// ============================================================================
// Tauri 命令
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
// 主程序
// ============================================================================

fn main() {
    println!("===========================================");
    println!("Go Magic Desktop v{}", env!("CARGO_PKG_VERSION"));
    println!("Mode: Process-Isolated");
    println!("===========================================");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .setup(|app| {
            let resource_dir = app
                .path()
                .resource_dir()
                .expect("Failed to get resource directory");
            let app_handle = app.handle().clone();

            println!("Resource directory: {:?}", resource_dir);

            match start_backend(&app_handle, &resource_dir) {
                Some((process, port)) => {
                    {
                        let mut guard = BACKEND_STATE.lock().unwrap();
                        *guard = Some(BackendState {
                            process,
                            port,
                            start_time: Instant::now(),
                        });
                    }

                    let server_url = format!("http://127.0.0.1:{}/", port);
                    println!("Creating window, URL: {}", server_url);

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

                    let _ = window.set_focus();

                    let _ = app_handle.emit(
                        "app-ready",
                        serde_json::json!({
                            "port": port,
                            "url": format!("http://127.0.0.1:{}", port)
                        }),
                    );

                    println!("Application ready");
                }
                None => {
                    eprintln!("Failed to start backend");
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
                println!("Close requested");
                stop_backend();
                window.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}