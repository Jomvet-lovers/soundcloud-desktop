//! SoundCloud 書き込みの縮退スタブ。
//!
//! 移植前の `direct/webview.rs` と同一の呼び出し形状
//! (`execute` / `execute_with` / `spawn_write` / `spawn_write_silent` /
//! `emit_sync_error`) を保つが、実実行はしない。wry writer (Phase 4) が
//! `WriteBackend` として接続されるまで、書き込みは `direct:sync-error`
//! イベントとして UI に通知される。ローカルストアへの即時反映は routes 側
//! で維持されるため、読み・ローカル操作は影響を受けない。

use serde_json::Value;

use super::events::EventBus;

pub const EVENT: &str = "direct:sync-error";

pub struct WriteOutcome {
    pub status: u16,
    pub payload: String,
    pub captcha: Option<String>,
}

/// Phase 4 で wry writer が実装する書き込み実体。
pub trait WriteBackend: Send + Sync {
    fn execute_blocking(
        &self,
        token: Option<&str>,
        method: &str,
        url: &str,
        body: Option<&Value>,
        extract: Option<&str>,
        solve: bool,
    ) -> Result<WriteOutcome, String>;
}

pub async fn execute(
    bus: &EventBus,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<WriteOutcome, String> {
    execute_with(bus, token, method, url, body, None, true).await
}

pub async fn execute_with(
    bus: &EventBus,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&Value>,
    _extract: Option<&str>,
    _solve: bool,
) -> Result<WriteOutcome, String> {
    let _ = token;
    let _ = body;
    emit_sync_error(
        bus,
        method,
        url,
        0,
        false,
        Some("writer unavailable until Phase 4 (wry writer)"),
    );
    Err("writer unavailable until Phase 4 (wry writer)".to_string())
}

pub fn emit_sync_error(
    bus: &EventBus,
    method: &str,
    url: &str,
    status: u16,
    captcha: bool,
    error: Option<&str>,
) {
    bus.emit(
        EVENT,
        &serde_json::json!({
            "method": method,
            "url": url,
            "status": status,
            "captcha": captcha,
            "error": error,
        }),
    );
}

pub fn spawn_write(
    bus: EventBus,
    token: String,
    method: &'static str,
    url: String,
    body: Option<Value>,
) {
    tokio::spawn(async move {
        match execute(&bus, Some(&token), method, &url, body.as_ref()).await {
            Ok(r) if (200..300).contains(&r.status) => {
                eprintln!("[writer] {method} {url} -> {}", r.status)
            }
            Ok(r) => {
                eprintln!(
                    "[writer] {method} {url} -> {} captcha={} body={}",
                    r.status,
                    r.captcha.is_some(),
                    r.payload.chars().take(200).collect::<String>()
                );
                emit_sync_error(&bus, method, &url, r.status, r.captcha.is_some(), None);
            }
            Err(e) => {
                eprintln!("[writer] {method} {url} failed: {e}");
                emit_sync_error(&bus, method, &url, 0, false, Some(&e));
            }
        }
    });
}

pub fn spawn_write_silent(
    bus: EventBus,
    token: String,
    method: &'static str,
    url: String,
    body: Option<Value>,
) {
    tokio::spawn(async move {
        match execute_with(&bus, Some(&token), method, &url, body.as_ref(), None, false).await {
            Ok(r) if (200..300).contains(&r.status) => {
                eprintln!("[writer] {method} {url} -> {}", r.status)
            }
            Ok(r) => eprintln!(
                "[writer] {method} {url} -> {} captcha={} (silent)",
                r.status,
                r.captcha.is_some()
            ),
            Err(e) => eprintln!("[writer] {method} {url} failed: {e} (silent)"),
        }
    });
}
