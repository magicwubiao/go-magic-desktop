//! Persisted window geometry: load, sanitize, save (debounced), and restore —
//! including the multi-monitor position clamping and the Windows-specific
//! single-step maximized reveal.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use tauri::AppHandle;

pub(crate) const DEFAULT_WINDOW_WIDTH: f64 = 1024.0;
pub(crate) const DEFAULT_WINDOW_HEIGHT: f64 = 800.0;
pub(crate) const MIN_WINDOW_WIDTH: f64 = 800.0;
pub(crate) const MIN_WINDOW_HEIGHT: f64 = 600.0;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub(crate) struct WindowState {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    /// Whether the window was maximized when the app was last closed.
    ///
    /// `width`/`height` deliberately stay pinned to the *restored* (pre-maximize)
    /// size, because a maximized window reports the monitor's size and restoring
    /// that as a plain window would leave it a few pixels short of the screen
    /// (the frame border) — which is exactly the "nearly full screen but
    /// offset" bug. `serde(default)` keeps older state files loading.
    #[serde(default)]
    pub(crate) maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

/// Clamps a persisted window state to sizes the window manager will accept.
///
/// Also swallows non-finite values: a corrupt state file can deserialize to
/// `f64::NAN` / `f64::INFINITY` (JSON has no NaN literal but `1e400` and
/// friends do), and an infinite width or NaN position would poison the whole
/// restore.
pub(crate) fn sanitize_window_state(mut state: WindowState) -> WindowState {
    state.width = finite_or(state.width, MIN_WINDOW_WIDTH);
    state.height = finite_or(state.height, MIN_WINDOW_HEIGHT);
    if !state.x.is_some_and(|v| v.is_finite()) {
        state.x = None;
    }
    if !state.y.is_some_and(|v| v.is_finite()) {
        state.y = None;
    }
    state
}

/// `v` if it is finite, `fallback` otherwise (`max`/`min` alone let
/// infinities through).
fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

fn get_window_state_path(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("window-state.json"))
}

/// A monitor's work area in *logical* pixels: (x, y, width, height).
type LogicalArea = (f64, f64, f64, f64);

fn logical_work_area(monitor: &tauri::Monitor) -> LogicalArea {
    let scale = monitor.scale_factor();
    let wa = monitor.work_area();
    (
        wa.position.x as f64 / scale,
        wa.position.y as f64 / scale,
        wa.size.width as f64 / scale,
        wa.size.height as f64 / scale,
    )
}

/// Clamps a persisted position to a monitor that can actually show it.
///
/// The monitor owning the top-left corner wins; when none does (the monitor it
/// lived on was unplugged, or the coordinates are bogus) the monitor with the
/// nearest work-area center is used, so the window comes back close to where
/// it was instead of being shoved onto the primary display.
pub(crate) fn adjust_position_for_screen(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    app_handle: &AppHandle,
) -> (f64, f64) {
    let Ok(monitors) = app_handle.available_monitors() else {
        return (x, y);
    };
    if monitors.is_empty() {
        return (x, y);
    }

    let mut chosen: Option<LogicalArea> = None;
    let mut nearest = f64::INFINITY;

    for area in monitors.iter().map(logical_work_area) {
        let (mx, my, mw, mh) = area;
        if x >= mx && x < mx + mw && y >= my && y < my + mh {
            chosen = Some(area);
            break;
        }
        let distance = (x - (mx + mw / 2.0)).hypot(y - (my + mh / 2.0));
        if distance < nearest {
            nearest = distance;
            chosen = Some(area);
        }
    }

    let Some((screen_x, screen_y, screen_width, screen_height)) = chosen else {
        return (x, y);
    };

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

    (new_x, new_y)
}

pub(crate) fn save_window_state(app_handle: &AppHandle) {
    let Some(path) = get_window_state_path(app_handle) else {
        return;
    };
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };

    // A maximized (or fullscreen) window reports the monitor's size in both
    // `inner_size` and `outer_position`, so persisting those would destroy the
    // restore geometry. Keep the previous size instead and only record the
    // `maximized` flag — that is what makes "maximize → close → reopen" land
    // exactly on the monitor again instead of a few pixels off.
    let maximized = window.is_maximized().unwrap_or(false);
    let fullscreen = window.is_fullscreen().unwrap_or(false);

    if maximized || fullscreen {
        let mut state = load_window_state(app_handle);
        state.maximized = maximized || fullscreen;
        write_window_state(&path, &state);
        return;
    }

    let scale_factor = window.scale_factor().unwrap_or(1.0);

    let (width, height) = match window.inner_size() {
        Ok(size) => {
            let logical: tauri::LogicalSize<u32> = size.to_logical(scale_factor);
            (logical.width as f64, logical.height as f64)
        }
        Err(_) => (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT),
    };

    let (x, y) = match window.outer_position() {
        Ok(pos) => {
            let logical: tauri::LogicalPosition<i32> = pos.to_logical(scale_factor);
            (Some(logical.x as f64), Some(logical.y as f64))
        }
        Err(_) => (None, None),
    };

    let state = WindowState {
        width: width.round(),
        height: height.round(),
        x: x.map(|v| v.round()),
        y: y.map(|v| v.round()),
        maximized: false,
    };

    write_window_state(&path, &state);
}

/// Writes the state atomically: the payload lands in a sibling `.tmp` file and
/// is renamed into place only when complete. A crash (or power loss) mid-write
/// can therefore never leave a truncated `window-state.json` behind — the
/// reader either sees the old state or the new one, never half of either.
fn write_window_state(path: &Path, state: &WindowState) {
    let Ok(json) = serde_json::to_string_pretty(state) else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let tmp = path.with_extension("json.tmp");
    let result = std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, path));

    if let Err(e) = result {
        eprintln!("Failed to save window state: {}", e);
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Debounce window for the resize/move persistence path.
const WINDOW_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// Raised whenever an un-persisted geometry change is observed.
static WINDOW_STATE_SAVE_DIRTY: AtomicBool = AtomicBool::new(false);
/// True while a debounce timer thread is sleeping.
static WINDOW_STATE_SAVE_TIMER_RUNNING: AtomicBool = AtomicBool::new(false);

/// Debounced [`save_window_state`] for the high-frequency Resized/Moved events.
///
/// Windows fires these events dozens of times per second while the user drags
/// the window; a synchronous `fs::write` per event wastes I/O and briefly
/// blocks the event loop on every tick. This coalesces each burst into a
/// single write [`WINDOW_STATE_SAVE_DEBOUNCE`] after the last event: one timer
/// thread waits it out, and any event landing during the wait only raises the
/// dirty flag.
///
/// The immediate paths (window close, 菜单退出) call [`save_window_state`]
/// directly, so a quit never depends on the debounce timer having fired.
pub(crate) fn schedule_window_state_save(app_handle: &AppHandle) {
    WINDOW_STATE_SAVE_DIRTY.store(true, Ordering::SeqCst);

    // `swap(true)` returning true means a timer thread is already sleeping;
    // it will observe the dirty flag before it exits, so no event is lost.
    if WINDOW_STATE_SAVE_TIMER_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }

    let app_handle = app_handle.clone();
    thread::spawn(move || {
        thread::sleep(WINDOW_STATE_SAVE_DEBOUNCE);
        WINDOW_STATE_SAVE_TIMER_RUNNING.store(false, Ordering::SeqCst);
        if WINDOW_STATE_SAVE_DIRTY.swap(false, Ordering::SeqCst) {
            save_window_state(&app_handle);
        }
    });
}

pub(crate) fn load_window_state(app_handle: &AppHandle) -> WindowState {
    let Some(path) = get_window_state_path(app_handle) else {
        return WindowState::default();
    };
    if !path.exists() {
        return WindowState::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<WindowState>(&content) {
            Ok(state) => sanitize_window_state(state),
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

/// Reveals `window` on screen already maximized, in a single step.
///
/// tao maximizes a window that is not visible yet with `ShowWindow(SW_MAXIMIZE)`
/// and then hides it again, which makes the window appear for an instant and
/// vanish (see the comment on the window builder in `main`). Asking the window
/// manager for `SW_SHOWMAXIMIZED` instead keeps the whole job in one call: the
/// first frame that is ever composited is already the final, maximized one, and
/// the window manager computes the frame and the monitor work area itself — so
/// this does not reintroduce the "monitor-sized rectangle, a few pixels off"
/// bug that persisting the maximized geometry used to cause.
///
/// Returns `false` when the native call could not be made, so the caller can
/// fall back to tao's own `maximize()`.
///
/// Once the tauri in use ships with tao >= 0.37.0 this helper and the
/// `cfg(not(windows))` split in `main` can both be dropped in favour of a plain
/// `maximized(window_state.maximized)` on the builder.
#[cfg(windows)]
pub(crate) fn reveal_maximized<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    // `ShowWindow` command: activates the window and displays it maximized.
    const SW_SHOWMAXIMIZED: i32 = 3;

    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(hwnd: *mut std::ffi::c_void, cmd_show: i32) -> i32;
    }

    let Ok(hwnd) = window.hwnd() else {
        return false;
    };

    // SAFETY: `hwnd` is the handle of the window that was just created and is
    // still alive here, and `ShowWindow` has no preconditions beyond the handle
    // being valid.
    unsafe {
        ShowWindow(hwnd.0, SW_SHOWMAXIMIZED);
    }

    true
}
