#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .setup(|_app| {
            start_backend();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                stop_backend();
                api.prevent_close();
                let _ = window.close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(target_os = "windows")]
fn start_backend() {
    let backend_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("go-magic.exe");

    if backend_path.exists() {
        let _ = Command::new(backend_path)
            .arg("server")
            .spawn();
    }
}

#[cfg(target_os = "macos")]
fn start_backend() {
    let backend_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go-magic");

    if backend_path.exists() {
        let _ = Command::new(backend_path)
            .arg("server")
            .spawn();
    }
}

#[cfg(target_os = "linux")]
fn start_backend() {
    let backend_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("go-magic");

    if backend_path.exists() {
        let _ = Command::new(backend_path)
            .arg("server")
            .spawn();
    }
}

fn stop_backend() {
}
