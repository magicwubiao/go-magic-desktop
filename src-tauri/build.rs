use std::process::Command;

/// Build script: version is derived entirely from git tags — no version field
/// in any config file is treated as the source of truth.
///
/// Exported env vars (read via `env!()` / `option_env!()` in main.rs):
///   - APP_VERSION   : version from `git describe --tags` (e.g. "1.2.3"), or "0.0.0-dev"
///   - GIT_COMMIT    : short git commit hash (empty if git unavailable)
///   - BUILD_TIME    : RFC3339 build timestamp
///   - BUILD_PROFILE : "debug" or "release"
fn main() {
    println!("cargo:rustc-env=APP_VERSION={}", git_version());
    println!("cargo:rustc-env=GIT_COMMIT={}", git_commit());
    println!("cargo:rustc-env=BUILD_TIME={}", now_utc());
    println!(
        "cargo:rustc-env=BUILD_PROFILE={}",
        if cfg!(debug_assertions) { "debug" } else { "release" }
    );

    println!("cargo:rerun-if-changed=../.git/HEAD");
    tauri_build::build()
}

/// Extract version from the most recent git tag: `git describe --tags --abbrev=0`.
/// Strips a leading `v` (so tag `v1.2.3` → version `1.2.3`).
/// Falls back to `0.0.0-dev` when there are no tags (local dev before first release).
fn git_version() -> String {
    let tag = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0.0.0-dev".to_string());

    tag.strip_prefix('v').unwrap_or(&tag).to_string()
}

fn git_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn now_utc() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
