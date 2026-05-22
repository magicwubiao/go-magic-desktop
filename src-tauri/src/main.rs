#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::{Command, Child};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

static BACKEND_PROCESS: Mutex<Option<Child>> = Mutex::new(None);
const SERVER_URL: &str = "http://localhost:5000";

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // 启动后端
            start_backend(app);
            
            // 等待后端就绪
            println!("Waiting for backend to start at {}...", SERVER_URL);
            
            let mut backend_ready = false;
            for i in 1..=20 {
                thread::sleep(Duration::from_millis(500));
                match reqwest::blocking::get(SERVER_URL) {
                    Ok(resp) => {
                        println!("Backend responded with status: {}", resp.status());
                        backend_ready = true;
                        break;
                    }
                    Err(e) => {
                        println!("Waiting... ({}/20) - {}", i, e);
                    }
                }
            }
            
            if !backend_ready {
                eprintln!("Warning: Backend may not be ready yet");
            }
            
            // 创建窗口并加载 URL
            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(SERVER_URL.parse().unwrap())
            )
            .title("Go Magic")
            .inner_size(1400.0, 900.0)
            .min_inner_size(1000.0, 700.0)
            .center()
            .focused(true)
            .build()
            .expect("Failed to create window");
            
            // 确保窗口显示在最前面
            let _ = window.set_always_on_top(true);
            let _ = window.set_always_on_top(false);
            let _ = window.set_focus();
            
            println!("Window created and shown, loading: {}", SERVER_URL);
            
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                stop_backend();
                let app_handle = window.app_handle();
                app_handle.exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn start_backend(app: &tauri::App) {
    let resource_dir = app.path().resource_dir().unwrap();
    
    let backend_name = if cfg!(target_os = "windows") {
        "go-magic.exe"
    } else {
        "go-magic"
    };

    let backend_path = resource_dir.join(backend_name);
    
    println!("Resource dir: {:?}", resource_dir);
    println!("Looking for backend at: {:?}", backend_path);

    if backend_path.exists() {
        println!("Backend found, starting...");
        
        match Command::new(&backend_path)
            .args(["server", "--port", "5000"])
            .current_dir(&resource_dir)
            .spawn() {
            Ok(child) => {
                println!("Backend process started with PID: {:?}", child.id());
                let mut process = BACKEND_PROCESS.lock().unwrap();
                *process = Some(child);
            }
            Err(e) => {
                eprintln!("Failed to start backend: {}", e);
            }
        }
    } else {
        eprintln!("Backend not found at: {:?}", backend_path);
        if let Ok(entries) = std::fs::read_dir(&resource_dir) {
            eprintln!("Resources directory contents:");
            for entry in entries.flatten() {
                eprintln!("  - {:?}", entry.path());
            }
        }
    }
}

fn stop_backend() {
    if let Ok(mut process) = BACKEND_PROCESS.lock() {
        if let Some(mut child) = process.take() {
            let _ = child.kill();
            println!("Backend stopped");
        }
    }
}
