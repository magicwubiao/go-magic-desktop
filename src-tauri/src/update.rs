//! The "check + prompt" update flow: the hidden update-check window reports a
//! version notice through IPC, this module validates it, shows the native
//! dialog (unless the same pair was already deferred), and persists the
//! deferral record.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, WebviewUrl};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::locales::{fill, shell_text};

/// Upper bound on the hidden update-check window's lifetime.
///
/// The page closes itself through `update_check_finished` once its check has
/// settled (found / nothing to do / failed). This bound exists so a hung
/// fetch — the page's own fetch timeout is 8s — can never leak a hidden
/// webview for the whole session.
const UPDATE_CHECK_MAX_LIFETIME: Duration = Duration::from_secs(20);
/// Release notes longer than this are cut off in the update dialog. A long
/// release body would otherwise render a dialog taller than the screen.
const UPDATE_NOTES_MAX_CHARS: usize = 600;
/// Whether the running update check was started manually from the 检查更新
/// menu item (vs the automatic launch-time check). Only a manual check gets
/// "up to date" / "failed" feedback dialogs — an automatic one stays silent
/// when there is nothing to report.
pub(crate) static MANUAL_UPDATE_CHECK: AtomicBool = AtomicBool::new(false);

/// Update notice handed over by the hidden update-check page.
///
/// The check itself lives in `src-tauri/splash/update-check.html`: it fetches
/// a static JSON manifest through the webview's network stack, which keeps the
/// shell's "no HTTP client, no TLS crate" design intact (see the security
/// notes in the README). This command is the native half of the flow — the
/// dialog and the browser hand-off it performs are fully native and
/// independent of the window that reported the notice.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateNotice {
    /// Version the running build reports (`get_app_info`).
    current_version: String,
    /// Version advertised by the remote update manifest.
    latest_version: String,
    /// Where to send the user; clamped to the official releases by
    /// [`official_download_url`] before anything opens a browser.
    download_url: String,
    /// Optional release notes shown inside the dialog.
    #[serde(default)]
    notes: String,
}

/// A "稍后" record: the user has already seen the update dialog for this
/// (current -> latest) pair and dismissed it.
///
/// The automatic launch-time check must not re-prompt for the same pair on
/// every start — once dismissed, it stays silent until either side of the pair
/// changes (a newer release ships, or the user actually upgraded, which moves
/// `current_version`). The manual 检查更新 menu item ignores this record and
/// always shows the dialog.
#[derive(serde::Serialize, serde::Deserialize)]
struct DeferredUpdateNotice {
    current_version: String,
    latest_version: String,
}

/// Path of the deferral record (next to the persisted window state).
fn deferred_notice_path(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("update-notice.json"))
}

/// Loads the deferral record; a missing or corrupt file means "no deferral".
fn load_deferred_notice(app_handle: &AppHandle) -> Option<DeferredUpdateNotice> {
    let path = deferred_notice_path(app_handle)?;
    let raw = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&raw) {
        Ok(deferred) => Some(deferred),
        Err(e) => {
            log::warn!("[update] ignoring corrupt update-notice.json: {}", e);
            None
        }
    }
}

/// Persists the deferral record; best-effort, failures are only logged (the
/// worst case is the dialog coming back on the next launch, which is harmless).
///
/// The write is atomic (tmp file + rename) so a crash mid-write can never
/// leave a truncated JSON behind.
fn save_deferred_notice(app_handle: &AppHandle, deferred: &DeferredUpdateNotice) {
    let Some(path) = deferred_notice_path(app_handle) else {
        return;
    };
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("[update] could not create {}: {}", dir.display(), e);
            return;
        }
    }
    match serde_json::to_string_pretty(deferred) {
        Ok(json) => {
            let tmp = path.with_extension("json.tmp");
            let result =
                std::fs::write(&tmp, json + "\n").and_then(|_| std::fs::rename(&tmp, &path));
            if let Err(e) = result {
                log::warn!("[update] could not write {}: {}", path.display(), e);
                let _ = std::fs::remove_file(&tmp);
            }
        }
        Err(e) => log::warn!("[update] could not serialize deferral: {}", e),
    }
}

/// The only destination the update dialog may open in the system browser: the
/// project's own GitHub releases.
///
/// The update manifest travels through shared CDNs (jsDelivr, GitHub raw)
/// whose caches have historically served stale content — and could in
/// principle serve tampered content. A poisoned manifest must therefore not be
/// able to point the "go download" button at an attacker's page, so the URL is
/// clamped to the official releases prefix; anything else falls back to the
/// canonical URL with a warning.
const OFFICIAL_RELEASES_URL: &str =
    "https://github.com/magicwubiao/go-magic-desktop/releases/latest";
/// Path prefix a manifest-provided download URL must sit under.
const OFFICIAL_RELEASES_PATH_PREFIX: &str = "/magicwubiao/go-magic-desktop/releases";

/// Validates a manifest-provided download URL against the official releases.
///
/// Returns `None` for anything that is not an `https://github.com` URL under
/// the project's releases path — the caller substitutes the canonical URL.
pub(crate) fn official_download_url(raw: &str) -> Option<tauri::Url> {
    let url = raw.parse::<tauri::Url>().ok()?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return None;
    }
    let path = url.path();
    if path != OFFICIAL_RELEASES_PATH_PREFIX
        && !path.starts_with(&format!("{OFFICIAL_RELEASES_PATH_PREFIX}/"))
    {
        return None;
    }
    Some(url)
}

/// Cuts release notes down to a dialog-friendly length, on a char boundary
/// (release bodies can be multibyte — the notes here are usually Chinese).
pub(crate) fn truncate_notes(notes: &str) -> String {
    if notes.chars().count() <= UPDATE_NOTES_MAX_CHARS {
        return notes.to_string();
    }
    let cut: String = notes.chars().take(UPDATE_NOTES_MAX_CHARS).collect();
    format!("{}…", cut.trim_end())
}

/// Shows a native "new version available" dialog; on confirmation opens the
/// download page in the system browser.
#[tauri::command]
pub(crate) fn notify_update_available(app_handle: AppHandle, notice: UpdateNotice) {
    // Only ever hand vetted https URLs to the OS browser: a manifest pointing
    // at `file://`, a custom scheme handler, or someone else's site is replaced
    // with the official releases URL rather than opened.
    let download_url = match official_download_url(&notice.download_url) {
        Some(url) => url,
        None => {
            log::warn!(
                "[update] manifest download URL {:?} is outside the official releases; \
                 using {} instead",
                notice.download_url,
                OFFICIAL_RELEASES_URL
            );
            OFFICIAL_RELEASES_URL
                .parse::<tauri::Url>()
                .expect("OFFICIAL_RELEASES_URL constant must be a valid URL")
        }
    };

    // The automatic launch-time check stays quiet for a pair the user already
    // dismissed; a manual 检查更新 always shows the dialog on purpose.
    if MANUAL_UPDATE_CHECK.load(Ordering::SeqCst) {
        log::info!("[update] manual check: prompting regardless of deferral");
    } else if load_deferred_notice(&app_handle).is_some_and(|deferred| {
        deferred.current_version == notice.current_version
            && deferred.latest_version == notice.latest_version
    }) {
        log::info!(
            "[update] skipping automatic prompt: {} -> {} was deferred by the user",
            notice.current_version,
            notice.latest_version
        );
        return;
    }

    let text = shell_text();

    let mut message = fill(
        text.dialog_update_body,
        &[&notice.latest_version, &notice.current_version],
    );
    let notes = notice.notes.trim();
    if !notes.is_empty() {
        message.push_str(&fill(text.dialog_update_notes, &[&truncate_notes(notes)]));
    }

    let deferred = DeferredUpdateNotice {
        current_version: notice.current_version.clone(),
        latest_version: notice.latest_version.clone(),
    };
    app_handle
        .dialog()
        .message(message)
        .title(text.dialog_update_title)
        .buttons(MessageDialogButtons::OkCancelCustom(
            text.dialog_update_go.to_string(),
            text.dialog_update_later.to_string(),
        ))
        .show(move |go_download| {
            // Record the pair whichever button was pressed: both mean "the
            // user has seen this update". Once they actually install it, the
            // running version changes and the record no longer matches, so
            // the next new release prompts again.
            save_deferred_notice(&app_handle, &deferred);
            if go_download {
                crate::open_external_link(&app_handle, download_url.as_str());
            }
        });
}

/// Called by the hidden update-check page once its check has settled:
///
/// - `"up-to-date"` — the manifest was fetched fine, but its version is not
///   newer than the running build.
/// - `"updated"` — a newer version was found and `notify_update_available`
///   already showed the update dialog.
/// - `"error"` — the check could not complete (network, bad manifest, ...).
///
/// Closes the hidden window. A *manual* check (菜单"检查更新...") also gets
/// explicit feedback for `up-to-date`/`error`; an automatic launch-time check
/// stays silent unless it found an update.
#[tauri::command]
pub(crate) fn update_check_finished(app_handle: AppHandle, status: String) {
    let manual = MANUAL_UPDATE_CHECK.swap(false, Ordering::SeqCst);

    if let Some(window) = app_handle.get_webview_window("update-check") {
        let _ = window.close();
    }

    if !manual {
        return;
    }

    match status.as_str() {
        "up-to-date" => {
            let text = shell_text();
            app_handle
                .dialog()
                .message(fill(text.dialog_check_up_to_date, &[env!("APP_VERSION")]))
                .title(text.dialog_check_updates_title)
                .buttons(MessageDialogButtons::Ok)
                .show(|_| {});
        }
        "error" => {
            let text = shell_text();
            app_handle
                .dialog()
                .message(text.dialog_check_error)
                .title(text.dialog_check_updates_title)
                .buttons(MessageDialogButtons::Ok)
                .show(|_| {});
        }
        // "updated": the update dialog is already on screen — nothing more to say.
        _ => {}
    }
}

/// Opens the hidden update-check window (`splash/update-check.html`) and arms
/// the 20s lifetime bound. Shared by the automatic launch-time check and the
/// "检查更新..." menu item; any window left over from a previous check is
/// destroyed first so the page always starts fresh.
pub(crate) fn spawn_update_check(app_handle: &AppHandle) -> Result<(), tauri::Error> {
    if let Some(existing) = app_handle.get_webview_window("update-check") {
        let _ = existing.destroy();
    }

    let result = WebviewWindowBuilder::new(
        app_handle,
        "update-check",
        WebviewUrl::App("update-check.html".into()),
    )
    .visible(false)
    .skip_taskbar(true)
    .build();

    if result.is_ok() {
        let handle = app_handle.clone();
        thread::spawn(move || {
            thread::sleep(UPDATE_CHECK_MAX_LIFETIME);
            if let Some(w) = handle.get_webview_window("update-check") {
                let _ = w.close();
            }
        });
    }

    result.map(|_| ())
}
