#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::{Command, Child};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::Manager;

static BACKEND_PROCESS: Mutex<Option<Child>> = Mutex::new(None);

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            start_backend(app);
            
            // 等待后端启动
            println!("Waiting for backend to start on http://localhost:5000...");
            
            // 等待服务器就绪
            for i in 1..=10 {
                thread::sleep(Duration::from_millis(500));
                if let Ok(resp) = reqwest::blocking::get("http://localhost:5000") {
                    if resp.status().is_success() {
                        println!("Backend is ready! Status: {}", resp.status());
                        break;
                    }
                }
                println!("Waiting... ({}/10)", i);
            }
            
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 阻止默认关闭行为，我们自己处理
                api.prevent_close();
                
                // 停止后端
                stop_backend();
                
                // 获取 app handle 并退出整个应用
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
    
    println!("Looking for backend at: {:?}", backend_path);
    println!("Resource dir: {:?}", resource_dir);

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
        // 列出 resources 目录内容
        if let Ok(entries) = std::fs::read_dir(&resource_dir) {
            eprintln!("Resources directory contents:");
            for entry in entries {
                if let Ok(entry) = entry {
                    eprintln!("  - {:?}", entry.path());
                }
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
