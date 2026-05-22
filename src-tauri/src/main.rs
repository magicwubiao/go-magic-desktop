#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::{Command, Child};
use std::sync::Mutex;
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
            Ok(())
        })
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                stop_backend();
                api.prevent_close();
                let _ = _window.close();
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

    if backend_path.exists() {
        match Command::new(&backend_path)
            .arg("server")
            .spawn() {
            Ok(child) => {
                let mut process = BACKEND_PROCESS.lock().unwrap();
                *process = Some(child);
                println!("Backend started: {:?}", backend_path);
            }
            Err(e) => {
                eprintln!("Failed to start backend: {}", e);
            }
        }
    } else {
        eprintln!("Backend not found at: {:?}", backend_path);
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
