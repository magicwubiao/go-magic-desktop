use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Build script: the version is derived entirely from git tags — no version
/// field in any config file is treated as the source of truth.
///
/// Exported env vars (read via `env!()` / `option_env!()` in main.rs):
///   - APP_VERSION   : version from `git describe --tags` (e.g. "1.2.3"), or "0.0.0-dev"
///   - GIT_COMMIT    : short git commit hash (empty if git is unavailable)
///   - GIT_BRANCH    : current branch name ("detached" on a detached HEAD)
///   - BUILD_TIME    : RFC3339 UTC build timestamp
///   - BUILD_PROFILE : "debug" or "release"
fn main() {
    println!("cargo:rustc-env=APP_VERSION={}", git_version());
    println!("cargo:rustc-env=GIT_COMMIT={}", git_commit());
    println!("cargo:rustc-env=GIT_BRANCH={}", git_branch());
    println!("cargo:rustc-env=BUILD_TIME={}", now_utc());
    println!(
        "cargo:rustc-env=BUILD_PROFILE={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    // Re-run when the checked-out revision changes, so the embedded version and
    // commit hash follow the working tree instead of going stale.
    for path in [
        "../.git/HEAD",
        "../.git/refs/heads",
        "../.git/refs/tags",
        "../.git/packed-refs",
    ] {
        if Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    tauri_build::build()
}

/// Runs `git` with the given arguments, returning trimmed stdout on success.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Version from the most recent git tag (`git describe --tags --abbrev=0`).
/// Strips a leading `v`, so tag `v1.2.3` becomes version `1.2.3`.
/// Falls back to `0.0.0-dev` when there are no tags (local dev before a release).
fn git_version() -> String {
    git(&["describe", "--tags", "--abbrev=0"])
        .map(|tag| tag.strip_prefix('v').unwrap_or(&tag).to_string())
        .unwrap_or_else(|| "0.0.0-dev".to_string())
}

fn git_commit() -> String {
    git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_default()
}

fn git_branch() -> String {
    match git(&["rev-parse", "--abbrev-ref", "HEAD"]) {
        // `--abbrev-ref` prints the literal string "HEAD" when detached.
        Some(branch) if branch == "HEAD" => "detached".to_string(),
        Some(branch) => branch,
        None => "unknown".to_string(),
    }
}

/// RFC3339 UTC timestamp computed with `std` only.
///
/// This previously shelled out to `date -u +%Y-%m-%dT%H:%M:%SZ`, which does not
/// exist on Windows — there `BUILD_TIME` silently degraded to "unknown".
fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);

    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (
        secs_of_day / 3_600,
        (secs_of_day % 3_600) / 60,
        secs_of_day % 60,
    );

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 -> (year, month, day).
/// Valid for the whole range that matters here and avoids pulling in a date crate.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year, [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}
