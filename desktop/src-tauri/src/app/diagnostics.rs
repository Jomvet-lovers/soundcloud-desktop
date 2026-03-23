use std::fs::{self, OpenOptions};
use std::io::Write;

use chrono::Local;
use tauri::{AppHandle, Manager};

const LOG_FILE_NAME: &str = "desktop.log";

fn log_file_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("failed to resolve app log dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create app log dir: {e}"))?;
    Ok(dir.join(LOG_FILE_NAME))
}

fn append_log_line(app: &AppHandle, line: &str) -> Result<(), String> {
    let path = log_file_path(app)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open log file: {e}"))?;

    writeln!(file, "{line}").map_err(|e| format!("failed to write log file: {e}"))?;
    Ok(())
}

fn format_log_line(level: &str, message: &str) -> String {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    format!("[{timestamp}] [{level}] {message}")
}

pub fn log_native(app: &AppHandle, level: &str, message: impl AsRef<str>) {
    let _ = append_log_line(app, &format_log_line(level, message.as_ref()));
}

pub fn mark_session_started(app: &AppHandle) {
    let _ = append_log_line(
        app,
        &format_log_line("INFO", "------------ SESSION STARTED -----------------"),
    );
    if let Ok(path) = log_file_path(app) {
        let _ = append_log_line(
            app,
            &format_log_line("INFO", &format!("Log file: {}", path.display())),
        );
    }
}

#[tauri::command]
pub fn diagnostics_log(app: AppHandle, level: String, message: String) -> Result<(), String> {
    append_log_line(&app, &format_log_line(&level, &message))
}
