//! Hidden writer for SoundCloud mutations (wry version).
//! Port of Tauri `direct/webview.rs`: runs `fetch()` on soundcloud.com in
//! the invisible window to dodge DataDome, collecting results through the
//! `scw` cookie. Falls back to a `direct:sync-error` stub when the wry
//! host is not running.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::events::EventBus;
use super::webhost::{self, HostProxy, WRITER_HOME};

pub const EVENT: &str = "direct:sync-error";
const READY_TIMEOUT: Duration = Duration::from_secs(25);
const FETCH_TIMEOUT: Duration = Duration::from_secs(25);
const SOLVE_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(150);

static SEQ: AtomicU64 = AtomicU64::new(1);
static LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct WriteOutcome {
    pub status: u16,
    pub payload: String,
    pub captcha: Option<String>,
}

/// Future seam for the wry writer (Phase 4 hookup point).
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

fn read_channel(cookies: &[(String, String)], seq: u64) -> Option<String> {
    let value = cookies
        .iter()
        .find(|(n, _)| n == "scw")
        .map(|(_, v)| v.clone())?;
    value.strip_prefix(&format!("{seq}:")).map(str::to_owned)
}

async fn wait_ready(host: &HostProxy) -> Result<(), String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let js = format!(
        r#"try{{if(document.readyState!=='loading'){{if(/(^|\.)soundcloud\.com$/.test(location.host)){{document.cookie='scw={seq}:ready;path=/';}}else{{location.href='{WRITER_HOME}';}}}}}}catch(_e){{}}"#
    );
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        host.writer_eval(&js).await?;
        tokio::time::sleep(POLL).await;
        let cookies = host.writer_cookies().await.unwrap_or_default();
        if read_channel(&cookies, seq).as_deref() == Some("ready") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("write webview never became ready".into());
        }
    }
}

async fn do_fetch(
    host: &HostProxy,
    token: Option<&str>,
    url: &str,
    method: &str,
    body: Option<&Value>,
    extract: Option<&str>,
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
    let mut init = serde_json::json!({
        "method": method,
        "credentials": "include",
        "headers": headers,
    });
    if let Some(b) = body {
        init["headers"]["Content-Type"] = serde_json::json!("application/json");
        init["body"] = serde_json::json!(b.to_string());
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
    host.writer_eval(&js).await?;

    let deadline = Instant::now() + FETCH_TIMEOUT;
    loop {
        tokio::time::sleep(POLL).await;
        let cookies = host.writer_cookies().await.unwrap_or_default();
        if let Some(rest) = read_channel(&cookies, seq) {
            let _ = host.writer_eval("document.cookie='scw=;Max-Age=0;path=/';").await;
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

fn is_challenge(status: u16, payload: &str) -> bool {
    (status == 401 || status == 403)
        && (payload.contains("captcha-delivery") || payload.contains("datadome"))
}

fn challenge_url(payload: &str) -> Option<String> {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|v| v.get("url").and_then(Value::as_str).map(str::to_owned))
}

async fn solve_challenge(host: &HostProxy, url: &str) -> Result<(), String> {
    let url_js = serde_json::to_string(url).map_err(|e| e.to_string())?;
    host.writer_eval(&format!("location.href={url_js}")).await?;
    host.writer_on_top(true).await;
    host.writer_show().await;
    host.writer_focus().await;

    let deadline = Instant::now() + SOLVE_TIMEOUT;
    loop {
        tokio::time::sleep(Duration::from_millis(750)).await;
        if host.writer_visible().await.map(|v| !v).unwrap_or(true) {
            host.writer_on_top(false).await;
            return Err("verification window closed".into());
        }
        match host.writer_url().await {
            Ok(current) => {
                if current == "soundcloud.com"
                    || current.ends_with(".soundcloud.com")
                    || current.contains("soundcloud.com/")
                {
                    host.writer_on_top(false).await;
                    return wait_ready(host).await;
                }
            }
            Err(_) => {
                host.writer_on_top(false).await;
                return Err("verification window closed".into());
            }
        }
        if Instant::now() >= deadline {
            host.writer_on_top(false).await;
            return Err("verification timed out".into());
        }
    }
}

fn no_host(bus: &EventBus, method: &str, url: &str) -> String {
    emit_sync_error(
        bus,
        method,
        url,
        0,
        false,
        Some("writer unavailable (wry host not running)"),
    );
    "writer unavailable (wry host not running)".to_string()
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
    extract: Option<&str>,
    solve: bool,
) -> Result<WriteOutcome, String> {
    let host = match webhost::ensure_host().await {
        Ok(h) => h,
        Err(_) => return Err(no_host(bus, method, url)),
    };
    let _lane = LANE.lock().await;
    wait_ready(&host).await?;

    let (status, payload) = do_fetch(&host, token, url, method, body, extract).await?;
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

    if let Err(e) = solve_challenge(&host, &challenge).await {
        host.writer_hide().await;
        return Err(e);
    }
    let (status, payload) = do_fetch(&host, token, url, method, body, extract).await?;
    host.writer_hide().await;
    let captcha = is_challenge(status, &payload).then_some(challenge);
    Ok(WriteOutcome {
        status,
        payload,
        captcha,
    })
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
