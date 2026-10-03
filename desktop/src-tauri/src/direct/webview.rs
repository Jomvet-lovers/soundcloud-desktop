//! Hidden WebView "writer" for SoundCloud mutations.
//!
//! DataDome (SoundCloud's bot protection) blocks plain HTTP clients on write
//! endpoints (`403`, `x-datadome: protected`), so mutations run as `fetch`
//! calls inside a real browser context that has soundcloud.com loaded. The
//! session cookie is injected with `set_cookie`. If DataDome answers with a
//! captcha, the window is shown so the user can solve it once; the write is
//! retried afterwards.

use std::time::Duration;

use serde_json::Value;
use tauri::Manager;

use crate::rt::{AppHandle, WebviewWindow};

pub const LABEL: &str = "sc-writer";
/// SoundCloud-side write sync is disabled: DataDome blocks every non-trusted
/// client (HTTP, TLS-impersonated HTTP, cookie replay, even an embedded
/// WebView2 whose challenge never leaves "verifying"). Writes stay local.
/// Flip to `true` to re-run the experiment.
pub const SYNC_ENABLED: bool = false;
const HOME: &str = "https://soundcloud.com/";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Serializes access to the single writer webview.
static WRITER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct WriteResult {
    pub status: u16,
    pub body: Value,
    pub captcha: Option<String>,
}

/// Create the hidden writer window (call from the main thread, e.g. setup).
pub fn ensure_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Some(w);
    }
    let url: tauri::Url = HOME.parse().ok()?;
    match tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::External(url))
        .title("SoundCloud session")
        .inner_size(520.0, 680.0)
        .visible(false)
        .skip_taskbar(true)
        .focused(false)
        .user_agent(UA)
        .build()
    {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("[writer] failed to create window: {e}");
            None
        }
    }
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

/// Inject the SoundCloud session cookie into the writer webview.
pub fn inject_session(wv: &WebviewWindow, token: &str) {
    if let Some(cookie) = session_cookie(token) {
        if let Err(e) = wv.set_cookie(cookie) {
            eprintln!("[writer] set_cookie failed: {e}");
        }
    }
}

async fn eval_value(wv: &WebviewWindow, js: &str) -> Result<Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = std::sync::Mutex::new(Some(tx));
    wv.eval_with_callback(js.to_string(), move |s| {
        if let Some(tx) = tx.lock().ok().and_then(|mut g| g.take()) {
            let _ = tx.send(s);
        }
    })
    .map_err(|e| e.to_string())?;
    let raw = rx.await.map_err(|_| "eval callback dropped".to_string())?;
    Ok(serde_json::from_str(&raw).unwrap_or(Value::Null))
}

async fn ensure_ready(app: &AppHandle, token: &str) -> Result<WebviewWindow, String> {
    let wv = ensure_window(app).ok_or_else(|| "writer window unavailable".to_string())?;
    if let Some(cookie) = session_cookie(token) {
        if let Err(e) = wv.set_cookie(cookie) {
            eprintln!("[writer] set_cookie failed: {e}");
        }
    }
    let on_home = wv
        .url()
        .map(|u| u.as_str().starts_with("https://soundcloud.com"))
        .unwrap_or(false);
    if !on_home {
        let url: tauri::Url = HOME.parse().map_err(|e| format!("{e}"))?;
        wv.navigate(url).map_err(|e| e.to_string())?;
        tokio::time::sleep(Duration::from_millis(3500)).await;
    }
    for _ in 0..20 {
        if let Ok(Value::String(s)) = eval_value(&wv, "document.readyState").await {
            if s == "complete" {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(wv)
}

async fn run_fetch(
    wv: &WebviewWindow,
    token: &str,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<WriteResult, String> {
    let method_s = serde_json::to_string(method).unwrap_or_else(|_| "\"GET\"".into());
    let url_s = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".into());
    let token_s = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".into());
    let body_s = body.map(|b| b.to_string()).unwrap_or_else(|| "null".into());
    let has_body = body.is_some();
    let js = format!(
        "window.__scw=null;(function(){{var init={{method:{method_s},credentials:'include',headers:{{'Accept':'application/json, text/javascript, */*; q=0.1','Authorization':'OAuth '+{token_s}}}}};var b={body_s};if({has_body}){{init.headers['Content-Type']='application/json';init.body=JSON.stringify(b);}}fetch({url_s},init).then(function(r){{return r.text().then(function(t){{window.__scw=JSON.stringify({{status:r.status,body:t}});}});}}).catch(function(e){{window.__scw=JSON.stringify({{status:-1,body:String(e)}});}});}})();"
    );
    wv.eval(js).map_err(|e| e.to_string())?;

    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let v = eval_value(wv, "window.__scw").await?;
        if let Value::String(s) = v {
            if s.is_empty() {
                continue;
            }
            let parsed: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
            let status = parsed.get("status").and_then(Value::as_u64).unwrap_or(0) as u16;
            let text = parsed.get("body").and_then(Value::as_str).unwrap_or("");
            let body_json: Value = serde_json::from_str(text).unwrap_or(Value::Null);
            let captcha = body_json
                .get("url")
                .and_then(Value::as_str)
                .filter(|u| u.contains("captcha-delivery"))
                .map(str::to_string);
            return Ok(WriteResult {
                status,
                body: body_json,
                captcha,
            });
        }
    }
    Err("writer timeout".into())
}

/// Show the window with a DataDome captcha and wait for the user to solve it.
async fn solve_captcha(app: &AppHandle, captcha: &str) -> bool {
    let Some(wv) = app.get_webview_window(LABEL) else {
        eprintln!("[writer] captcha: writer window missing");
        return false;
    };
    let Ok(url) = captcha.parse::<tauri::Url>() else {
        eprintln!("[writer] captcha: bad url");
        return false;
    };
    eprintln!("[writer] captcha required: showing window for the user");
    let _ = wv.set_always_on_top(true);
    let _ = wv.show();
    let _ = wv.set_focus();
    let _ = wv.navigate(url);
    for _ in 0..480 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(u) = wv.url() {
            if !u.as_str().contains("captcha-delivery") {
                eprintln!("[writer] captcha left the challenge page, retrying");
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let _ = wv.set_always_on_top(false);
                let _ = wv.hide();
                return true;
            }
        }
    }
    eprintln!("[writer] captcha timed out");
    let _ = wv.set_always_on_top(false);
    let _ = wv.hide();
    false
}

/// Perform a SoundCloud mutation inside the writer webview.
pub async fn write(
    app: &AppHandle,
    token: &str,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<WriteResult, String> {
    let _guard = WRITER_LOCK.lock().await;
    for attempt in 0..2 {
        let wv = ensure_ready(app, token).await?;
        let result = run_fetch(&wv, token, method, url, body).await?;
        if let Some(captcha) = result.captcha.clone() {
            if attempt == 0 && solve_captcha(app, &captcha).await {
                continue;
            }
        }
        return Ok(result);
    }
    Err("writer retry exhausted".into())
}

/// Fire-and-forget sync of a mutation. Never blocks the local API response.
pub fn spawn_write(
    app: AppHandle,
    token: String,
    method: &'static str,
    url: String,
    body: Option<Value>,
) {
    if !SYNC_ENABLED {
        return;
    }
    tokio::spawn(async move {
        match write(&app, &token, method, &url, body.as_ref()).await {
            Ok(r) if (200..300).contains(&r.status) => {
                eprintln!("[writer] {method} {url} -> {}", r.status)
            }
            Ok(r) => eprintln!(
                "[writer] {method} {url} -> {} captcha={} body={}",
                r.status,
                r.captcha.is_some(),
                r.body.to_string().chars().take(120).collect::<String>()
            ),
            Err(e) => eprintln!("[writer] {method} {url} failed: {e}"),
        }
    });
}
