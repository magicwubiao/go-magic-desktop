//! Shell localization: every user-facing string the Tauri shell shows natively
//! (menu bar items, native dialogs, startup error messages).
//!
//! The web UI owns its own vue-i18n bundles (`web/src/locales`, following the
//! user's in-app choice); the shell only needs this handful of strings and
//! follows the **OS language** instead (`detect_shell_text`) — the same
//! convention native desktop apps use.
//!
//! All string fields are plain `&'static str`. Templated ones keep `{}`
//! placeholders that go through [`fill`], because `format!` requires a string
//! *literal* as its template and cannot take a runtime string.

use std::sync::OnceLock;

pub(crate) struct ShellText {
    // 编辑(E) menu
    pub(crate) edit_menu: &'static str,
    pub(crate) edit_cut: &'static str,
    pub(crate) edit_copy: &'static str,
    pub(crate) edit_paste: &'static str,
    pub(crate) edit_select_all: &'static str,
    // 窗口(W) menu
    pub(crate) window_menu: &'static str,
    pub(crate) window_minimize: &'static str,
    pub(crate) window_maximize: &'static str,
    pub(crate) window_pin: &'static str,
    pub(crate) window_reload: &'static str,
    // 帮助(H) menu
    pub(crate) help_menu: &'static str,
    pub(crate) help_check_updates: &'static str,
    pub(crate) help_open_logs: &'static str,
    pub(crate) help_open_data: &'static str,
    pub(crate) help_restart_backend: &'static str,
    pub(crate) help_open_docs: &'static str,
    // 关于(A) menu
    pub(crate) about_menu: &'static str,
    pub(crate) about_app: &'static str,
    pub(crate) quit_app: &'static str,
    pub(crate) quit_app_accelerator: &'static str,
    // Startup / runtime dialogs
    pub(crate) dialog_startup_error: &'static str,
    pub(crate) dialog_startup_failed: &'static str,
    pub(crate) dialog_resource_dir_error: &'static str,
    pub(crate) dialog_backend_not_found: &'static str,
    pub(crate) dialog_invalid_url: &'static str,
    pub(crate) dialog_backend_not_ready: &'static str,
    pub(crate) dialog_backend_not_ready_restart: &'static str,
    pub(crate) dialog_service_stopped_title: &'static str,
    pub(crate) dialog_service_stopped: &'static str,
    // About dialog
    pub(crate) dialog_about_title: &'static str,
    pub(crate) dialog_about_body: &'static str,
    // Check-for-updates dialogs
    pub(crate) dialog_check_updates_title: &'static str,
    pub(crate) dialog_check_spawn_failed: &'static str,
    pub(crate) dialog_check_up_to_date: &'static str,
    pub(crate) dialog_check_error: &'static str,
    // Update-available dialog
    pub(crate) dialog_update_title: &'static str,
    pub(crate) dialog_update_body: &'static str,
    pub(crate) dialog_update_notes: &'static str,
    pub(crate) dialog_update_go: &'static str,
    pub(crate) dialog_update_later: &'static str,
    // `BackendError` Display strings — surfaced through startup error dialogs
    // and logs, so they localize like every other user-facing string.
    pub(crate) error_no_port: &'static str,
    pub(crate) error_backend_not_found: &'static str,
    pub(crate) error_spawn_failed: &'static str,
    pub(crate) error_health_check_timeout: &'static str,
}

pub(crate) const ZH_TEXT: ShellText = ShellText {
    edit_menu: "编辑(&E)",
    edit_cut: "剪切(&T)",
    edit_copy: "复制(&C)",
    edit_paste: "粘贴(&P)",
    edit_select_all: "全选(&A)",
    window_menu: "窗口(&W)",
    window_minimize: "最小化(&N)",
    window_maximize: "最大化(&M)",
    window_pin: "窗口置顶",
    window_reload: "刷新页面(&R)",
    help_menu: "帮助(&H)",
    help_check_updates: "检查更新...",
    help_open_logs: "打开日志目录",
    help_open_data: "打开数据目录",
    help_restart_backend: "重启后端服务",
    help_open_docs: "使用文档",
    about_menu: "关于(&A)",
    about_app: "关于 Go Magic",
    quit_app: "退出 Go Magic",
    quit_app_accelerator: "Alt+F4",
    dialog_startup_error: "启动错误",
    dialog_startup_failed: "应用启动失败",
    dialog_resource_dir_error: "获取资源目录失败：{}",
    dialog_backend_not_found: "未找到后端程序\n请检查资源目录：{}",
    dialog_invalid_url: "后端地址无效 {}：{}",
    dialog_backend_not_ready: "后端未在 {} 秒内就绪。\n请查看日志文件了解详情。",
    dialog_backend_not_ready_restart: "后端重启后未在 {} 秒内就绪。",
    dialog_service_stopped_title: "Go Magic 服务已停止",
    dialog_service_stopped: "Go Magic 服务意外退出（退出码 {}）。\n\n自动重启 {} 次均失败，已放弃拉起。\n请重启应用，并查看日志文件中后端最后的输出。",
    dialog_about_title: "关于 Go Magic",
    dialog_about_body: "Go Magic Desktop\n版本 {}（{}）\n构建时间：{}",
    dialog_check_updates_title: "检查更新",
    dialog_check_spawn_failed: "检查更新失败，请稍后重试。",
    dialog_check_up_to_date: "当前已是最新版本（{}）。",
    dialog_check_error: "检查更新失败，请检查网络连接后重试。",
    dialog_update_title: "发现新版本",
    dialog_update_body: "新版本 {} 已发布（当前版本 {}）。\n\n是否前往下载页面？",
    dialog_update_notes: "\n\n更新内容：\n{}",
    dialog_update_go: "前往下载",
    dialog_update_later: "稍后再说",
    error_no_port: "未找到可用端口",
    error_backend_not_found: "未找到后端程序",
    error_spawn_failed: "启动后端失败：{}",
    error_health_check_timeout: "后端健康检查超时，请确认后端是否正常工作",
};

pub(crate) const EN_TEXT: ShellText = ShellText {
    edit_menu: "&Edit",
    edit_cut: "Cu&t",
    edit_copy: "&Copy",
    edit_paste: "&Paste",
    edit_select_all: "Select &All",
    window_menu: "&Window",
    window_minimize: "&Minimize",
    window_maximize: "Ma&ximize",
    window_pin: "Always on Top",
    window_reload: "&Refresh",
    help_menu: "&Help",
    help_check_updates: "Check for Updates...",
    help_open_logs: "Open Log Folder",
    help_open_data: "Open Data Folder",
    help_restart_backend: "Restart Backend Service",
    help_open_docs: "Documentation",
    about_menu: "&About",
    about_app: "About Go Magic",
    quit_app: "Exit Go Magic",
    quit_app_accelerator: "Alt+F4",
    dialog_startup_error: "Startup Error",
    dialog_startup_failed: "Application Startup Failed",
    dialog_resource_dir_error: "Failed to get the resource directory: {}",
    dialog_backend_not_found: "Backend executable not found\nPlease check the resource directory: {}",
    dialog_invalid_url: "Invalid backend URL {}: {}",
    dialog_backend_not_ready: "The backend did not become ready within {}s.\nPlease check the log file for details.",
    dialog_backend_not_ready_restart: "The backend did not become ready within {}s after a restart.",
    dialog_service_stopped_title: "Go Magic Service Stopped",
    dialog_service_stopped: "The Go Magic service stopped unexpectedly (exit code {}).\n\nAutomatic restarts failed {} times, so it was not brought back.\nPlease restart the application and check the log file for the backend's last output.",
    dialog_about_title: "About Go Magic",
    dialog_about_body: "Go Magic Desktop\nVersion {} ({})\nBuild time: {}",
    dialog_check_updates_title: "Check for Updates",
    dialog_check_spawn_failed: "Update check failed. Please try again later.",
    dialog_check_up_to_date: "You're up to date ({}).",
    dialog_check_error: "Update check failed. Check your network connection and try again.",
    dialog_update_title: "Update Available",
    dialog_update_body: "Version {} is available (current {}).\n\nGo to the download page?",
    dialog_update_notes: "\n\nWhat's new:\n{}",
    dialog_update_go: "Download",
    dialog_update_later: "Later",
    error_no_port: "No available port found",
    error_backend_not_found: "Backend executable not found",
    error_spawn_failed: "Failed to start backend: {}",
    error_health_check_timeout:
        "Backend health check timed out, please verify if backend is working correctly",
};

/// Minimal `format!` substitute: fills `{}` placeholders left to right. Needed
/// because `format!` requires a string *literal* as its template, while the
/// localized tables above are runtime `&'static str`s.
pub(crate) fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_string();
    for arg in args {
        if let Some(pos) = out.find("{}") {
            out.replace_range(pos..pos + 2, arg);
        }
    }
    out
}

/// Picks the shell text by OS language: Chinese locales get the Chinese table,
/// everything else falls back to English — mirroring the web UI's zh-or-en
/// model. Accessed anywhere via [`shell_text`]; resolved once per process.
fn detect_shell_text() -> &'static ShellText {
    let zh = sys_locale::get_locale()
        .map(|l| l.to_ascii_lowercase().starts_with("zh"))
        .unwrap_or(false);
    if zh {
        &ZH_TEXT
    } else {
        &EN_TEXT
    }
}

/// Process-wide shell text, resolved lazily on first use.
pub(crate) fn shell_text() -> &'static ShellText {
    static SHELL_TEXT: OnceLock<&'static ShellText> = OnceLock::new();
    SHELL_TEXT.get_or_init(detect_shell_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_placeholders_left_to_right() {
        assert_eq!(fill("版本 {}（{}）", &["1.0", "abc12345"]), "版本 1.0（abc12345）");
        assert_eq!(
            fill("Update within {}s", &["60"]),
            "Update within 60s"
        );
    }

    #[test]
    fn fill_without_placeholders_returns_the_template() {
        assert_eq!(fill("检查更新", &[]), "检查更新");
        assert_eq!(fill("no args here", &["ignored"]), "no args here");
    }

    #[test]
    fn shell_text_tables_cover_every_field() {
        // A missing (empty) field in either table silently renders blank menus
        // and dialogs, so assert the non-empty invariant for all of them.
        let tables: [&ShellText; 2] = [&ZH_TEXT, &EN_TEXT];
        for table in tables {
            assert!(!table.edit_menu.is_empty());
            assert!(!table.edit_cut.is_empty());
            assert!(!table.edit_copy.is_empty());
            assert!(!table.edit_paste.is_empty());
            assert!(!table.edit_select_all.is_empty());
            assert!(!table.window_menu.is_empty());
            assert!(!table.window_minimize.is_empty());
            assert!(!table.window_maximize.is_empty());
            assert!(!table.window_pin.is_empty());
            assert!(!table.window_reload.is_empty());
            assert!(!table.help_menu.is_empty());
            assert!(!table.help_check_updates.is_empty());
            assert!(!table.help_open_logs.is_empty());
            assert!(!table.help_open_data.is_empty());
            assert!(!table.help_restart_backend.is_empty());
            assert!(!table.help_open_docs.is_empty());
            assert!(!table.about_menu.is_empty());
            assert!(!table.about_app.is_empty());
            assert!(!table.quit_app.is_empty());
            assert!(!table.quit_app_accelerator.is_empty());
            assert!(!table.dialog_startup_error.is_empty());
            assert!(!table.dialog_startup_failed.is_empty());
            assert!(!table.dialog_resource_dir_error.is_empty());
            assert!(!table.dialog_backend_not_found.is_empty());
            assert!(!table.dialog_invalid_url.is_empty());
            assert!(!table.dialog_backend_not_ready.is_empty());
            assert!(!table.dialog_backend_not_ready_restart.is_empty());
            assert!(!table.dialog_service_stopped_title.is_empty());
            assert!(!table.dialog_service_stopped.is_empty());
            assert!(!table.dialog_about_title.is_empty());
            assert!(!table.dialog_about_body.is_empty());
            assert!(!table.dialog_check_updates_title.is_empty());
            assert!(!table.dialog_check_spawn_failed.is_empty());
            assert!(!table.dialog_check_up_to_date.is_empty());
            assert!(!table.dialog_check_error.is_empty());
            assert!(!table.dialog_update_title.is_empty());
            assert!(!table.dialog_update_body.is_empty());
            assert!(!table.dialog_update_notes.is_empty());
            assert!(!table.dialog_update_go.is_empty());
            assert!(!table.dialog_update_later.is_empty());
            assert!(!table.error_no_port.is_empty());
            assert!(!table.error_backend_not_found.is_empty());
            assert!(!table.error_spawn_failed.is_empty());
            assert!(!table.error_health_check_timeout.is_empty());
        }
    }

    #[test]
    fn placeholder_counts_match_between_tables() {
        // `fill` replaces `{}` left to right with no arity check, so a table
        // whose placeholders drifted apart would render half-filled messages.
        // Count the placeholders in every templated field and require the two
        // tables to agree.
        let pairs: [(&str, &str); 7] = [
            (ZH_TEXT.dialog_resource_dir_error, EN_TEXT.dialog_resource_dir_error),
            (ZH_TEXT.dialog_backend_not_found, EN_TEXT.dialog_backend_not_found),
            (ZH_TEXT.dialog_invalid_url, EN_TEXT.dialog_invalid_url),
            (ZH_TEXT.dialog_backend_not_ready, EN_TEXT.dialog_backend_not_ready),
            (
                ZH_TEXT.dialog_backend_not_ready_restart,
                EN_TEXT.dialog_backend_not_ready_restart,
            ),
            (ZH_TEXT.dialog_service_stopped, EN_TEXT.dialog_service_stopped),
            (ZH_TEXT.dialog_about_body, EN_TEXT.dialog_about_body),
            (ZH_TEXT.error_spawn_failed, EN_TEXT.error_spawn_failed),
        ];
        for (zh, en) in pairs {
            assert_eq!(zh.matches("{}").count(), en.matches("{}").count(), "{zh:?} vs {en:?}");
        }
        // Two-arg pair check for the update dialog (body has two, notes one).
        assert_eq!(ZH_TEXT.dialog_update_body.matches("{}").count(), 2);
        assert_eq!(EN_TEXT.dialog_update_body.matches("{}").count(), 2);
        assert_eq!(ZH_TEXT.dialog_update_notes.matches("{}").count(), 1);
        assert_eq!(EN_TEXT.dialog_update_notes.matches("{}").count(), 1);
        assert_eq!(ZH_TEXT.dialog_check_up_to_date.matches("{}").count(), 1);
        assert_eq!(EN_TEXT.dialog_check_up_to_date.matches("{}").count(), 1);
    }
}
