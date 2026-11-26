#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Go Magic Desktop - Process Isolation Mode
//!
//! This shell is deliberately thin: it spawns the bundled `go-magic` backend as
//! a child process, shows a bundled splash page until the backend's health
//! check answers, then points the webview at `http://127.0.0.1:<port>` and
//! shuts the backend down again when the window closes.
//!
//! Module layout:
//! - [`backend`]      — spawn / supervise / restart / stop the child process
//! - [`window_state`] — persisted window geometry (load, sanitize, debounce)
//! - [`update`]       — the hidden update-check window and its dialog flow
//! - [`locales`]      — shell text tables (menus, dialogs) per OS language
//! - `main.rs`        — Tauri glue: builder, menu, event handlers, IPC commands

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

mod backend;
mod locales;
mod update;
mod window_state;

// Re-exports keep the crate-root namespace flat, so `tests.rs` (which imports
// `use crate::*`) keeps working unchanged across the module split.
pub(crate) use backend::*;
pub(crate) use locales::{fill, shell_text};
pub(crate) use update::*;
pub(crate) use window_state::*;

// Tests live in their own module.
#[cfg(test)]
mod tests;

// ============================================================================
// Small UI / navigation helpers
// ============================================================================

fn show_error_dialog(app_handle: &AppHandle, title: &str, message: &str) {
    eprintln!("{}: {}", title, message);
    app_handle
        .dialog()
        .message(message)
        .title(title)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

pub(crate) fn open_external_link(_app_handle: &AppHandle, url: &str) {
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

/// URL filter shared by `on_navigation` and `on_new_window`: only the local
/// backend and the shell's own pages may load in the webview, everything else
/// goes to the system browser.
///
/// The tauri host must match *exactly* (`tauri.localhost`): a suffix check
/// such as `ends_with("tauri.localhost")` would also admit a lookalike like
/// `eviltauri.localhost`.
pub(crate) fn is_shell_or_backend_url(url: &tauri::Url) -> bool {
    let host = url.host_str().unwrap_or("");
    host == "127.0.0.1"
        || host == "localhost"
        || host == "tauri.localhost"
        || url.scheme() == "tauri"
}

/// Filesystem locations that may hold the backend binary, in priority order.
///
/// Both `resources/*` (which lands in `<resource_dir>/resources/`) and a
/// flattened layout (binary at the resource root) are covered, because the
/// packaging config and the bundler differ per platform.
pub(crate) fn backend_search_dirs(resource_dir: &Path) -> Vec<PathBuf> {
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

pub(crate) fn find_backend_path(resource_dir: &Path) -> Option<PathBuf> {
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

    // Last resort for *development* only: let the OS resolve the binary from
    // PATH. A release build must never do this — executing whatever `go-magic`
    // happens to be on the user's PATH (during the probe and then for the
    // whole session) would run an unknown backend version behind the shell's
    // back. A shipped app that cannot find its bundled binary reports the
    // error dialog instead.
    #[cfg(debug_assertions)]
    {
        #[cfg(target_os = "windows")]
        const PATH_BINARY: &str = "go-magic.exe";
        #[cfg(not(target_os = "windows"))]
        const PATH_BINARY: &str = "go-magic";

        if Command::new(PATH_BINARY).arg("--version").output().is_ok() {
            println!("Found backend in PATH: {}", PATH_BINARY);
            return Some(PathBuf::from(PATH_BINARY));
        }
    }

    eprintln!("Backend executable not found. Searched paths:");
    for (i, path) in searched.iter().enumerate() {
        eprintln!("  {}: {:?} (exists: {})", i + 1, path, path.exists());
    }

    None
}

// ============================================================================
// Tauri Commands
// ============================================================================

#[tauri::command]
fn get_backend_port() -> Option<u16> {
    current_backend_port()
}

/// Shared restart entry point: fires `backend-restarting` and restarts the
/// backend on a background thread. Used by both the `restart_backend_cmd` IPC
/// command and the 帮助(H) → 重启后端服务 menu item.
fn trigger_backend_restart(app_handle: &AppHandle) {
    let Ok(resource_dir) = app_handle.path().resource_dir() else {
        log::error!("[backend] restart failed: resource directory is unavailable");
        return;
    };

    // A restart takes seconds; never block the IPC thread on it.
    let _ = app_handle.emit("backend-restarting", serde_json::json!({}));

    // The thread needs an owned handle: a spawned closure must be `'static`,
    // and a `&AppHandle` borrowed from the caller would not outlive it.
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
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
fn restart_backend_cmd(app_handle: AppHandle) {
    trigger_backend_restart(&app_handle);
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

/// Health check surfaced to the frontend.
///
/// Runs on the async runtime's blocking pool. A synchronous command would
/// execute on the main thread, and a single slow probe (up to
/// [`HEALTH_PROBE_TIMEOUT`], two seconds) would freeze the whole UI while the
/// frontend waits for the answer.
#[tauri::command]
async fn check_backend_health_cmd(port: Option<u16>) -> bool {
    // Only probe a port we actually own. The old `unwrap_or(5000)` fallback
    // could answer "healthy" for a *different* app instance's backend (or any
    // other server that happened to sit on 5000) whenever the shell's own
    // registration was missing — a misleading result in the exact situation
    // (backend gone) the check exists for.
    let Some(port) = port.or_else(current_backend_port) else {
        log::warn!("[backend] health check requested but no backend is running");
        return false;
    };

    match tauri::async_runtime::spawn_blocking(move || check_backend_health(port)).await {
        Ok(healthy) => healthy,
        Err(e) => {
            log::warn!("[backend] health check task failed: {}", e);
            false
        }
    }
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
        // MUST be the first plugin: it takes a process-wide mutex before
        // anything else runs. When a second instance is launched, the callback
        // below fires in the *first* instance, and the second process exits
        // immediately — it never spawns its own backend, so the two shells can
        // no longer fight over the backend port and the shared `~/.magic` data
        // directory (which manifested as one window reloading every few
        // seconds while its supervisor "successfully" restarted a backend that
        // could never bind).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Bring the already-running instance to the foreground so the
            // user understands why nothing else opened.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
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

            let text = shell_text();

            let resource_dir = match app.path().resource_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    show_error_dialog(
                        &app_handle,
                        text.dialog_startup_error,
                        &fill(text.dialog_resource_dir_error, &[&e.to_string()]),
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
                        BackendError::BackendNotFound => fill(
                            text.dialog_backend_not_found,
                            &[&format!("{:?}", resource_dir)],
                        ),
                        other => other.to_string(),
                    };

                    eprintln!("Failed to start backend: {}", error_msg);
                    show_error_dialog(&app_handle, text.dialog_startup_failed, &error_msg);
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
                std::thread::spawn(move || supervise_backend(app_handle, resource_dir));
            }

            let server_url = format!("http://127.0.0.1:{}/", port);
            let backend_url: tauri::Url = match server_url.parse() {
                Ok(url) => url,
                Err(e) => {
                    let msg = fill(text.dialog_invalid_url, &[&server_url, &e.to_string()]);
                    eprintln!("{}", msg);
                    show_error_dialog(&app_handle, text.dialog_startup_failed, &msg);
                    return Err("Invalid backend URL".into());
                }
            };

            // The window opens on a bundled splash page (src-tauri/splash,
            // embedded via `frontendDist` in tauri.conf.json) instead of the
            // backend URL: at this point the backend has not bound its port
            // yet, and pointing the webview straight at it makes WebView2
            // render its own "can't reach this page" error for a moment. That
            // flash was most visible on the very first launch, where first-run
            // initialisation keeps the backend from listening for a few
            // seconds; on later launches the port usually binds before the
            // webview's first navigation, hiding the race. The health-check
            // thread below navigates to the real URL once the backend answers.
            let splash_url = WebviewUrl::App("index.html".into());

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

            let builder = WebviewWindowBuilder::new(app, "main", splash_url)
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
                        // Allow local backend URLs and the shell's own splash page
                        // (`http://tauri.localhost` on Windows, `tauri://localhost`
                        // on macOS/Linux); block other navigation and open it in
                        // the system browser instead.
                        if is_shell_or_backend_url(&url) {
                            return true;
                        }
                        open_external_link(&app_handle_clone, url.as_str());
                        false
                    }
                })
                .on_new_window({
                    let app_handle_clone = app_handle.clone();
                    move |url, _features| {
                        // Intercept target="_blank" links and open in system browser
                        if is_shell_or_backend_url(&url) {
                            return NewWindowResponse::Allow;
                        }
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

            // ----------------------------------------------------------------
            // Native menu bar (labels come from `shell_text()`, which follows
            // the OS language; see `locales.rs`):
            //
            //   编辑/Edit(E)  窗口/Window(W)  帮助/Help(H)  关于/About(A)
            //
            // Predefined items run natively; on Windows cut/copy/paste/
            // select_all and minimize/maximize are supported, while undo/redo/
            // fullscreen are not (docs.rs: "Windows / Linux: Unsupported") —
            // those are deliberately left out instead of shipping no-ops.
            // Custom items dispatch through `.on_menu_event` below. Built
            // best-effort: a menu error is logged, never fatal to startup.
            let app_menu: tauri::Result<Menu<tauri::Wry>> = (|| {
                let handle = &app_handle;
                let text = shell_text();

                // 编辑(E): standard clipboard actions, native accelerators.
                let edit_submenu = Submenu::with_id(handle, "edit_menu", text.edit_menu, true)?;
                edit_submenu.append(&PredefinedMenuItem::cut(handle, Some(text.edit_cut))?)?;
                edit_submenu.append(&PredefinedMenuItem::copy(handle, Some(text.edit_copy))?)?;
                edit_submenu.append(&PredefinedMenuItem::paste(handle, Some(text.edit_paste))?)?;
                edit_submenu.append(&PredefinedMenuItem::separator(handle)?)?;
                edit_submenu.append(&PredefinedMenuItem::select_all(
                    handle,
                    Some(text.edit_select_all),
                )?)?;

                // 窗口(W): native window commands plus custom toggles.
                let window_submenu =
                    Submenu::with_id(handle, "window_menu", text.window_menu, true)?;
                window_submenu.append(&PredefinedMenuItem::minimize(
                    handle,
                    Some(text.window_minimize),
                )?)?;
                window_submenu.append(&PredefinedMenuItem::maximize(
                    handle,
                    Some(text.window_maximize),
                )?)?;
                window_submenu.append(&PredefinedMenuItem::separator(handle)?)?;
                // 置顶 is a toggle: a clone of the checkbox item is kept in
                // managed state so the event handler can keep the checkmark in
                // sync with the window's actual always-on-top flag.
                let pin_item = CheckMenuItem::with_id(
                    handle,
                    "toggle_pin",
                    text.window_pin,
                    true,
                    false,
                    None::<&str>,
                )?;
                app_handle.manage(Mutex::new(Some(pin_item.clone())));
                window_submenu.append(&pin_item)?;
                window_submenu.append(&PredefinedMenuItem::separator(handle)?)?;
                window_submenu.append(&MenuItem::with_id(
                    handle,
                    "reload_page",
                    text.window_reload,
                    true,
                    Some("F5"),
                )?)?;

                // 帮助(H): update check, diagnostics, external links.
                let help_submenu = Submenu::with_id(handle, "help_menu", text.help_menu, true)?;
                help_submenu.append(&MenuItem::with_id(
                    handle,
                    "check_updates",
                    text.help_check_updates,
                    true,
                    None::<&str>,
                )?)?;
                help_submenu.append(&PredefinedMenuItem::separator(handle)?)?;
                help_submenu.append(&MenuItem::with_id(
                    handle,
                    "open_log_dir",
                    text.help_open_logs,
                    true,
                    None::<&str>,
                )?)?;
                help_submenu.append(&MenuItem::with_id(
                    handle,
                    "open_data_dir",
                    text.help_open_data,
                    true,
                    None::<&str>,
                )?)?;
                help_submenu.append(&MenuItem::with_id(
                    handle,
                    "restart_backend",
                    text.help_restart_backend,
                    true,
                    None::<&str>,
                )?)?;
                help_submenu.append(&PredefinedMenuItem::separator(handle)?)?;
                help_submenu.append(&MenuItem::with_id(
                    handle,
                    "open_docs",
                    text.help_open_docs,
                    true,
                    None::<&str>,
                )?)?;

                // 关于(A): about dialog + quit.
                let about_submenu = Submenu::with_id(handle, "about_menu", text.about_menu, true)?;
                about_submenu.append(&MenuItem::with_id(
                    handle,
                    "about_app",
                    text.about_app,
                    true,
                    None::<&str>,
                )?)?;
                about_submenu.append(&PredefinedMenuItem::separator(handle)?)?;
                about_submenu.append(&MenuItem::with_id(
                    handle,
                    "quit_app",
                    text.quit_app,
                    true,
                    Some(text.quit_app_accelerator),
                )?)?;

                Menu::with_items(
                    handle,
                    &[
                        &edit_submenu,
                        &window_submenu,
                        &help_submenu,
                        &about_submenu,
                    ],
                )
            })();
            match app_menu {
                Ok(menu) => {
                    // App-wide menu: `AppHandle::set_menu` takes the bare
                    // `Menu` (not an `Option<Menu>`) and is the only variant
                    // that works on macOS, where a menu cannot be attached to
                    // a single window. Windows without an explicit menu — the
                    // main window here — pick it up automatically.
                    if let Err(e) = app_handle.set_menu(menu) {
                        log::warn!("[menu] failed to attach menu: {}", e);
                    }
                }
                Err(e) => log::warn!("[menu] failed to build menu: {}", e),
            }

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

            // Wait for the backend off the UI thread, then swap the splash page
            // for the real UI: the window initially shows the bundled loading
            // page, and this navigation replaces it once the server answers.
            {
                let app_handle = app_handle.clone();
                let window = window.clone();

                std::thread::spawn(move || {
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
                    // Stop the backend on a background thread and exit only
                    // once it is gone: `stop_backend` blocks for up to the
                    // full grace period, and doing that synchronously here
                    // would (a) delay the error dialog and (b) risk the app
                    // exiting before the backend was actually stopped.
                    BACKEND_SHUTTING_DOWN.store(true, std::sync::atomic::Ordering::SeqCst);
                    let exit_handle = app_handle.clone();
                    std::thread::spawn(move || {
                        stop_backend();
                        exit_handle.exit(1);
                    });

                    let msg = fill(
                        shell_text().dialog_backend_not_ready,
                        &[&HEALTH_CHECK_TIMEOUT.as_secs().to_string()],
                    );
                    log::error!("[backend] {}", msg);
                    let _ = app_handle.emit("backend-error", serde_json::json!({ "message": msg }));
                    show_error_dialog(&app_handle, shell_text().dialog_startup_failed, &msg);
                    app_handle.exit(1);
                });
            }

            // ----------------------------------------------------------------
            // Hidden update-check window (方案 C: 版本检查 + 跳转下载页).
            //
            // The check lives in src-tauri/splash/update-check.html: it
            // fetches a static JSON manifest through the webview's network
            // stack — keeping the shell's "no HTTP client, no TLS crate"
            // design intact — and invokes `notify_update_available` only
            // when the manifest advertises a newer version.
            //
            // Why a hidden window instead of the splash page: on warm starts
            // the backend binds its port almost immediately and the
            // health-check thread navigates the main window away within the
            // first second, which would cut any in-flight splash-page fetch
            // short. This window is invisible and independent of the main
            // window's navigation, so the check runs to completion on every
            // launch without delaying startup.
            // Launch-time update check: best-effort by design — a failure here
            // must never block the app from starting.
            if let Err(e) = spawn_update_check(&app_handle) {
                log::warn!("[update] could not open the update-check window: {}", e);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_backend_port,
            restart_backend_cmd,
            get_app_info,
            check_backend_health_cmd,
            notify_update_available,
            update_check_finished,
        ])
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "about_app" => {
                    let text = shell_text();
                    let about = fill(
                        text.dialog_about_body,
                        &[
                            env!("APP_VERSION"),
                            option_env!("GIT_COMMIT").unwrap_or("unknown"),
                            option_env!("BUILD_TIME").unwrap_or("unknown"),
                        ],
                    );
                    app.dialog()
                        .message(about)
                        .title(text.dialog_about_title)
                        .buttons(MessageDialogButtons::Ok)
                        .show(|_| {});
                }
                "check_updates" => {
                    // The hidden window reports back through
                    // `update_check_finished`, which shows the result dialog
                    // because the manual flag is set.
                    MANUAL_UPDATE_CHECK.store(true, std::sync::atomic::Ordering::SeqCst);
                    if let Err(e) = spawn_update_check(app) {
                        MANUAL_UPDATE_CHECK.store(false, std::sync::atomic::Ordering::SeqCst);
                        log::warn!("[update] manual check failed to open window: {}", e);
                        let text = shell_text();
                        app.dialog()
                            .message(text.dialog_check_spawn_failed)
                            .title(text.dialog_check_updates_title)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                }
                "toggle_pin" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let pinned = !window.is_always_on_top().unwrap_or(false);
                        if let Err(e) = window.set_always_on_top(pinned) {
                            log::warn!("[menu] failed to toggle always-on-top: {}", e);
                            return;
                        }
                        // Keep the checkmark in sync with the window state.
                        // Bind the guard explicitly: `state.lock()` as an
                        // if-let scrutinee extends its temporary past `state`'s
                        // drop (E0597), and the codebase treats poisoning as
                        // recoverable (see RESTART_GUARD).
                        let state = app.state::<Mutex<Option<CheckMenuItem<tauri::Wry>>>>();
                        let guard = match state.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        if let Some(item) = guard.as_ref() {
                            let _ = item.set_checked(pinned);
                        }
                    }
                }
                "reload_page" => {
                    if let Some(window) = app.get_webview_window("main") {
                        if let Err(e) = window.eval("location.reload()") {
                            log::warn!("[menu] failed to reload page: {}", e);
                        }
                    }
                }
                "open_log_dir" | "open_data_dir" => {
                    let dir = if event.id().as_ref() == "open_log_dir" {
                        app.path().app_log_dir()
                    } else {
                        app.path().app_data_dir()
                    };
                    match dir {
                        Ok(dir) => {
                            // The log dir may not exist yet on a fresh install;
                            // opening a missing folder would silently no-op.
                            if let Err(e) = std::fs::create_dir_all(&dir) {
                                log::warn!("[menu] failed to create {:?}: {}", dir, e);
                            }
                            if let Err(e) = open::that(&dir) {
                                log::warn!("[menu] failed to open {:?}: {}", dir, e);
                            }
                        }
                        Err(e) => log::warn!("[menu] failed to resolve directory: {}", e),
                    }
                }
                "restart_backend" => trigger_backend_restart(app),
                "open_docs" => {
                    if let Err(e) = open::that("https://magictech.cc/docs.html") {
                        log::warn!("[menu] failed to open docs: {}", e);
                    }
                }
                "quit_app" => {
                    // Same path as closing the main window: persist geometry
                    // first (the menu path skips CloseRequested), then stop the
                    // backend off the UI thread and exit once it is gone.
                    save_window_state(app);
                    shutdown_and_exit(app);
                }
                _ => {}
            }
        })
        .on_window_event(|window, event| {
            let app_handle = window.app_handle().clone();
            // Auxiliary windows (the hidden update-check webview) neither own
            // the window state nor the app's lifetime; only "main" does.
            if window.label() != "main" {
                return;
            }
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    save_window_state(&app_handle);
                    // Sets the shutdown flag first, then stops the backend on a
                    // background thread and exits once it is gone — so the
                    // supervisor cannot restart the backend mid-exit, and the
                    // UI never freezes on the up-to-10s grace/force/reap path.
                    shutdown_and_exit(&app_handle);
                }
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                    // Debounced: these events fire in bursts while dragging, and
                    // a synchronous write per event briefly blocks the loop.
                    schedule_window_state_save(&app_handle);
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}
