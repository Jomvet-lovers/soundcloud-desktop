//! Tauri の `app_data_dir` / `app_cache_dir` / `app_log_dir` の代替。
//!
//! `PRODUCT_NAME` を `tauri.conf.json` の productName
//! (`soundcloud-desktop`) と同一にしているため、既存の
//! `auth_session.json` / `direct_store.json` 等はそのまま引き継がれる。

use std::path::PathBuf;

pub const PRODUCT_NAME: &str = "soundcloud-desktop";

pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join(PRODUCT_NAME))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn app_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .map(|d| d.join(PRODUCT_NAME))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn app_log_dir() -> PathBuf {
    app_data_dir().join("logs")
}

pub fn ensure_dirs() -> std::io::Result<()> {
    for dir in [app_data_dir(), app_cache_dir(), app_log_dir()] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}
