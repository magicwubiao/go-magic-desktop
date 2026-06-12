use std::fs;
use std::path::PathBuf;

fn main() {
    // 从 package.json 读取版本号（唯一版本源）
    let pkg_json: PathBuf = ["..", "package.json"].iter().collect();
    let version = fs::read_to_string(&pkg_json)
        .ok()
        .and_then(|s| {
            let after_label = s.split("\"version\"").nth(1)?;
            let after_quote = after_label.split('"').nth(1)?;
            let next_quote = after_quote.find('"').unwrap_or(after_quote.len());
            Some(after_quote[..next_quote].trim().to_string())
        })
        .unwrap_or_else(|| "0.0.0".to_string());

    // 导出给 Rust 代码：用 APP_VERSION 替代 CARGO_PKG_VERSION
    println!("cargo:rustc-env=APP_VERSION={}", version);

    // package.json 变化时重新运行
    println!("cargo:rerun-if-changed=../package.json");

    tauri_build::build()
}