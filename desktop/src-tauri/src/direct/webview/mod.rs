//! Hidden WebView "writer" for SoundCloud mutations.
//!
//! DataDome scores the whole client 驕ｯ・ｶ郢晢ｽｻTLS/HTTP2 fingerprint plus its sensor
//! cookies 驕ｯ・ｶ郢晢ｽｻso writes sent by an HTTP client are challenged. A `fetch()` from
//! a page on https://soundcloud.com presents the real web app's fingerprint,
//! so writes pass. The already signed-in login webview is reused (a fresh
//! incognito jar gets hard-challenged on Windows), falling back to a
//! persistent writer window when the login window is gone.
//!
//! Remote pages have no Tauri IPC, so results travel through a `scw` cookie
//! the page sets and `cookies_for_url` reads. A sequence number guards against
//! stale reads and a lane mutex keeps the channel single-writer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::rt::{AppHandle, WebviewWindow};

pub const LABEL: &str = "sc-writer";
pub const EVENT: &str = "direct:sync-error";
pub(super) const HOME: &str = "https://soundcloud.com/";
pub(super) const READY_TIMEOUT: Duration = Duration::from_secs(25);
pub(super) const FETCH_TIMEOUT: Duration = Duration::from_secs(25);
const SOLVE_TIMEOUT: Duration = Duration::from_secs(180);
pub(super) const POLL: Duration = Duration::from_millis(150);

pub(super) static SEQ: AtomicU64 = AtomicU64::new(1);
pub(super) static LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

mod probe;
mod sniff;
mod upload;
pub use probe::{execute_form, page_fetch_probe};
pub use sniff::{attr_play, hot_replay, sniff_official_play};
pub use upload::{UploadOutcome, abort_upload, do_upload};

pub struct WriteOutcome {
    pub status: u16,
    pub payload: String,
    pub captcha: Option<String>,
}

pub fn ensure_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Some(w);
    }
    let url: Url = HOME.parse().ok()?;
    match WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("SoundCloud session")
        .inner_size(520.0, 680.0)
        .visible(false)
        .skip_taskbar(true)
        .focused(false)
        .build()
    {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("[writer] failed to create window: {e}");
            None
        }
    }
}

pub(super) fn current_writer_window(app: &AppHandle) -> Option<WebviewWindow> {
    // The login watcher reads `oauth_token`; injecting into its window would
    // look like a fresh sign-in, so writers use the dedicated window instead.
    if super::login::is_watching() {
        app.get_webview_window(LABEL)
    } else {
        app.get_webview_window(super::login::LABEL)
            .or_else(|| app.get_webview_window(LABEL))
    }
}

pub(super) fn writer_window(app: &AppHandle) -> Option<WebviewWindow> {
    current_writer_window(app).or_else(|| ensure_window(app))
}

fn session_cookie(token: &str) -> Option<tauri::webview::Cookie<'static>> {
    Some(
        tauri::webview::Cookie::build(("oauth_token", token.to_string()))
            .domain(".soundcloud.com")
            .path("/")
            .secure(true)
            .http_only(true)
            .build(),
    )
}

pub fn inject_session(wv: &WebviewWindow, token: &str) {
    if let Some(cookie) = session_cookie(token) {
        if let Err(e) = wv.set_cookie(cookie) {
            eprintln!("[writer] set_cookie failed: {e}");
        }
    }
}

pub(super) fn eval(window: &WebviewWindow, js: &str) -> Result<(), String> {
    window.eval(js).map_err(|e| e.to_string())
}

pub(super) fn cookies_safe(
    window: &WebviewWindow,
    url: Url,
) -> Option<Vec<tauri::webview::Cookie<'static>>> {
    let w = window.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || w.cookies_for_url(url)))
        .ok()
        .and_then(Result::ok)
}

pub(super) fn read_channel_named(window: &WebviewWindow, name: &str, seq: u64) -> Option<String> {
    let url: Url = HOME.parse().ok()?;
    let cookies = cookies_safe(window, url)?;
    let value = cookies
        .iter()
        .find(|c| c.name() == name)
        .map(|c| c.value().to_string())?;
    value.strip_prefix(&format!("{seq}:")).map(str::to_owned)
}

pub(super) fn read_channel(window: &WebviewWindow, seq: u64) -> Option<String> {
    read_channel_named(window, "scw", seq)
}

/// Snapshot every cookie in the writer jar as `(name, value)` pairs. Upload

pub(super) async fn wait_ready(window: &WebviewWindow) -> Result<(), String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let js = format!(
        r#"try{{if(document.readyState!=='loading'){{if(/(^|\.)soundcloud\.com$/.test(location.host)){{document.cookie='scw={seq}:ready;path=/';}}else{{location.href='{HOME}';}}}}}}catch(_e){{}}"#
    );
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        eval(window, &js)?;
        tokio::time::sleep(POLL).await;
        if read_channel(window, seq).as_deref() == Some("ready") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("write webview never became ready".into());
        }
    }
}

/// Writer window that is ready for `do_fetch`, with the oauth cookie aligned
/// to the token sent in the `Authorization` header.
pub(super) async fn prepared_window(app: &AppHandle, token: Option<&str>) -> Result<WebviewWindow, String> {
    let window = writer_window(app).ok_or_else(|| "writer window unavailable".to_string())?;
    wait_ready(&window).await?;
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        inject_session(&window, t);
    }
    Ok(window)
}

pub(super) async fn do_fetch(
    window: &WebviewWindow,
    token: Option<&str>,
    url: &str,
    method: &str,
    body: Option<&Value>,
    extract: Option<&str>,
    content_type: Option<&str>,
    extra_headers: Option<&serde_json::Map<String, Value>>,
) -> Result<(u16, String), String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let url_js = serde_json::to_string(url).map_err(|e| e.to_string())?;

    let mut headers = serde_json::Map::new();
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        headers.insert(
            "Authorization".into(),
            serde_json::json!(format!("OAuth {t}")),
        );
    }
    if let Some(extra) = extra_headers {
        for (k, v) in extra {
            headers.insert(k.clone(), v.clone());
        }
    }
    let mut init = serde_json::json!({
        "method": method,
        "credentials": "include",
        "headers": headers,
    });
    if let Some(b) = body {
        // A string body is sent verbatim (form-encoded diagnostics); objects
        // keep the historical JSON serialization.
        let ct = content_type.unwrap_or(match b {
            Value::String(_) => "application/x-www-form-urlencoded",
            _ => "application/json",
        });
        init["headers"]["Content-Type"] = serde_json::json!(ct);
        init["body"] = match b {
            Value::String(raw) => serde_json::json!(raw),
            _ => serde_json::json!(b.to_string()),
        };
    }

    let extract_js = match extract {
        Some(e) => format!("try{{t=({e})(t);}}catch(_e){{t='';}}"),
        None => String::new(),
    };
    let js = format!(
        r#"fetch({url_js},{init}).then(async function(r){{
            var t='';try{{t=await r.text();}}catch(_e){{}}
            {extract_js}
            document.cookie='scw={seq}:'+r.status+':'+encodeURIComponent(String(t).slice(0,800))+';path=/';
        }}).catch(function(e){{
            document.cookie='scw={seq}:ERR:'+encodeURIComponent(String(e&&e.message||e).slice(0,200))+';path=/';
        }});"#
    );
    eval(window, &js)?;

    let deadline = Instant::now() + FETCH_TIMEOUT;
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            if let Some(msg) = rest.strip_prefix("ERR:") {
                return Err(format!("page fetch: {msg}"));
            }
            let (status, encoded) = rest.split_once(':').unwrap_or((rest.as_str(), ""));
            let status: u16 = status.parse().map_err(|_| format!("bad result: {rest}"))?;
            let payload = urlencoding::decode(encoded)
                .map(|s| s.into_owned())
                .unwrap_or_default();
            return Ok((status, payload));
        }
        if Instant::now() >= deadline {
            return Err("write fetch timed out".into());
        }
    }
}

/// Diagnostics only: fetch `url` from the writer page and report just the
/// status, byte size, and content type. `do_fetch` returns the body as text,
/// which cannot confirm binary payloads (audio bytes), so the upload probes
/// use this instead. Shares the single-writer `scw` channel and holds the
pub(super) fn json_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

pub(crate) fn is_challenge(status: u16, payload: &str) -> bool {
    (status == 401 || status == 403)
        && (payload.contains("captcha-delivery") || payload.contains("datadome"))
}

fn challenge_url(payload: &str) -> Option<String> {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|v| v.get("url").and_then(Value::as_str).map(str::to_owned))
}

async fn solve_challenge(window: &WebviewWindow, url: &str) -> Result<(), String> {
    let url_js = serde_json::to_string(url).map_err(|e| e.to_string())?;
    eval(window, &format!("location.href={url_js}"))?;
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();

    let deadline = Instant::now() + SOLVE_TIMEOUT;
    loop {
        tokio::time::sleep(Duration::from_millis(750)).await;
        if window.is_visible().is_ok_and(|v| !v) {
            let _ = window.set_always_on_top(false);
            return Err("verification window closed".into());
        }
        match window.url() {
            Ok(current) => {
                let back_home = current
                    .host_str()
                    .is_some_and(|h| h == "soundcloud.com" || h.ends_with(".soundcloud.com"));
                if back_home {
                    let _ = window.set_always_on_top(false);
                    return wait_ready(window).await;
                }
            }
            Err(_) => {
                let _ = window.set_always_on_top(false);
                return Err("verification window closed".into());
            }
        }
        if Instant::now() >= deadline {
            let _ = window.set_always_on_top(false);
            return Err("verification timed out".into());
        }
    }
}

pub async fn execute(
    app: &AppHandle,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<WriteOutcome, String> {
    execute_with(app, token, method, url, body, None, true).await
}

/// TEMP-DIAG(history): page fetch with explicit Content-Type, optional extra
/// headers, and optional auth suppression (token None => no Authorization).
pub async fn execute_full(
    app: &AppHandle,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&Value>,
    content_type: Option<&str>,
    extra: Option<&serde_json::Map<String, Value>>,
) -> Result<WriteOutcome, String> {
    // NOTE: intentionally lock-free (unlike execute/execute_with): diag-only
    // concurrent path so replays can run DURING a sniff-held lane. The scw
    // cookie channel is seq-guarded, so interleaved evals stay separated.
    let window = prepared_window(app, token).await?;
    let (status, payload) =
        do_fetch(&window, token, url, method, body, None, content_type, extra).await?;
    Ok(WriteOutcome { status, payload, captcha: None })
}

pub async fn execute_with(
    app: &AppHandle,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&Value>,
    extract: Option<&str>,
    solve: bool,
) -> Result<WriteOutcome, String> {
    let _lane = LANE.lock().await;
    let window = prepared_window(app, token).await?;

    let (status, payload) = do_fetch(&window, token, url, method, body, extract, None, None).await?;
    if !is_challenge(status, &payload) {
        return Ok(WriteOutcome {
            status,
            payload,
            captcha: None,
        });
    }

    let Some(challenge) = challenge_url(&payload) else {
        return Ok(WriteOutcome {
            status,
            payload,
            captcha: None,
        });
    };
    if challenge.contains("t=bv") || !solve {
        return Ok(WriteOutcome {
            status,
            payload,
            captcha: Some(challenge),
        });
    }

    if let Err(e) = solve_challenge(&window, &challenge).await {
        let _ = window.hide();
        return Err(e);
    }
    let (status, payload) = do_fetch(&window, token, url, method, body, extract, None, None).await?;
    let _ = window.hide();
    let captcha = is_challenge(status, &payload).then_some(challenge);
    Ok(WriteOutcome {
        status,
        payload,
        captcha,
    })
}

/// Poll the writer page until its URL contains `needle` and the document is
/// complete. Uses the shared `scw` cookie channel with a dedicated seq.
pub(super) async fn wait_for_page(window: &WebviewWindow, needle: &str, timeout: Duration) -> Result<(), String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let deadline = Instant::now() + timeout;
    loop {
        let _ = eval(
            window,
            &format!(
                r#"try{{document.cookie='scw={seq}:'+encodeURIComponent(location.href+'|'+document.readyState)+';path=/';}}catch(_e){{}}"#
            ),
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Some(rest) = read_channel(window, seq) {
            let info = urlencoding::decode(&rest)
                .map(|s| s.into_owned())
                .unwrap_or_default();
            if info.contains(needle) && info.contains("complete") {
                let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!("page never settled ({needle})"));
        }
    }
}

/// JS installed into the writer page by the history sniffer: force-mutes media
/// (autoplay policy) and records api-v2 traffic into `window.__sniffed`.
/// Re-runnable: never wipes an existing array, wraps fetch/XHR only once.

pub fn emit_sync_error(
    app: &AppHandle,
    method: &str,
    url: &str,
    status: u16,
    captcha: bool,
    error: Option<&str>,
) {
    let _ = app.emit(
        EVENT,
        serde_json::json!({
            "method": method,
            "url": url,
            "status": status,
            "captcha": captcha,
            "error": error,
        }),
    );
}

pub fn spawn_write(
    app: AppHandle,
    token: String,
    method: &'static str,
    url: String,
    body: Option<Value>,
) {
    tokio::spawn(async move {
        match execute(&app, Some(&token), method, &url, body.as_ref()).await {
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
                emit_sync_error(&app, method, &url, r.status, r.captcha.is_some(), None);
            }
            Err(e) => {
                eprintln!("[writer] {method} {url} failed: {e}");
                emit_sync_error(&app, method, &url, 0, false, Some(&e));
            }
        }
    });
}

