//! Hidden WebView "writer" for SoundCloud mutations.
//!
//! DataDome scores the whole client 窶・TLS/HTTP2 fingerprint plus its sensor
//! cookies 窶・so writes sent by an HTTP client are challenged. A `fetch()` from
//! a page on https://soundcloud.com presents the real web app's fingerprint,
//! so writes pass. The already signed-in login webview is reused (a fresh
//! incognito jar gets hard-challenged on Windows), falling back to a
//! persistent writer window when the login window is gone.
//!
//! Remote pages have no Tauri IPC, so results travel through a `scw` cookie
//! the page sets and `cookies_for_url` reads. A sequence number guards against
//! stale reads and a lane mutex keeps the channel single-writer.

// Upload entry points are wired into Tauri commands by the upload commit.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::rt::{AppHandle, WebviewWindow};

pub const LABEL: &str = "sc-writer";
pub const EVENT: &str = "direct:sync-error";
const HOME: &str = "https://soundcloud.com/";
const READY_TIMEOUT: Duration = Duration::from_secs(25);
const FETCH_TIMEOUT: Duration = Duration::from_secs(25);
const SOLVE_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(150);
const UPLOAD_CHANNEL: &str = "scu";
/// Max characters per completion cookie (encoded form), mirroring `CH` in
/// `build_upload_js`. Keeps every cookie far below the ~4096-byte jar limit.
const UPLOAD_CHUNK_MAX: usize = 2000;
const UPLOAD_PROGRESS_EVENT: &str = "track:upload-progress";
const UPLOAD_STALL: Duration = Duration::from_secs(120);
/// Fail fast when the writer page never produces a single event, which is what
/// an invalid cookie name (silently dropped by the browser) looks like.
const UPLOAD_START_TIMEOUT: Duration = Duration::from_secs(120);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

static SEQ: AtomicU64 = AtomicU64::new(1);
static LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static UPLOAD_LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct WriteOutcome {
    pub status: u16,
    pub payload: String,
    pub captcha: Option<String>,
}

pub struct UploadOutcome {
    pub status: u16,
    pub payload: String,
}

/// Event read from the `scu<seq>` cookie while an upload is in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadEvent {
    /// `START`: the audio fetch finished; the XHR is about to send.
    Started,
    Progress(u8),
    /// `DONE:<n>`: the response body was split into `<n>` chunk cookies.
    Chunks { total: usize },
    Done { status: u16, payload: String },
    Failed(String),
    Aborted,
}

fn decode_channel_value(value: &str) -> String {
    urlencoding::decode(value)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

/// Parse a `scu<seq>` value (the sequence lives in the cookie name).
pub fn parse_upload_event(value: &str) -> Option<UploadEvent> {
    if value == "ABORTED" {
        return Some(UploadEvent::Aborted);
    }
    if value == "START" {
        return Some(UploadEvent::Started);
    }
    if let Some(message) = value.strip_prefix("ERR:") {
        return Some(UploadEvent::Failed(decode_channel_value(message)));
    }
    if let Some(percent) = value.strip_prefix("P:") {
        return percent.parse::<u8>().ok().map(UploadEvent::Progress);
    }
    if let Some(total) = value.strip_prefix("DONE:") {
        return total
            .parse::<usize>()
            .ok()
            .map(|total| UploadEvent::Chunks { total });
    }
    let (status, encoded) = match value.split_once(':') {
        Some((status, encoded)) => (status, Some(encoded)),
        None => (value, None),
    };
    let status = status.parse::<u16>().ok()?;
    let payload = encoded.map(decode_channel_value).unwrap_or_default();
    Some(UploadEvent::Done { status, payload })
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

fn current_writer_window(app: &AppHandle) -> Option<WebviewWindow> {
    // The login watcher reads `oauth_token`; injecting into its window would
    // look like a fresh sign-in, so writers use the dedicated window instead.
    if super::login::is_watching() {
        app.get_webview_window(LABEL)
    } else {
        app.get_webview_window(super::login::LABEL)
            .or_else(|| app.get_webview_window(LABEL))
    }
}

fn writer_window(app: &AppHandle) -> Option<WebviewWindow> {
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

fn eval(window: &WebviewWindow, js: &str) -> Result<(), String> {
    window.eval(js).map_err(|e| e.to_string())
}

fn cookies_safe(
    window: &WebviewWindow,
    url: Url,
) -> Option<Vec<tauri::webview::Cookie<'static>>> {
    let w = window.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || w.cookies_for_url(url)))
        .ok()
        .and_then(Result::ok)
}

fn read_channel_named(window: &WebviewWindow, name: &str, seq: u64) -> Option<String> {
    let url: Url = HOME.parse().ok()?;
    let cookies = cookies_safe(window, url)?;
    let value = cookies
        .iter()
        .find(|c| c.name() == name)
        .map(|c| c.value().to_string())?;
    value.strip_prefix(&format!("{seq}:")).map(str::to_owned)
}

fn read_channel(window: &WebviewWindow, seq: u64) -> Option<String> {
    read_channel_named(window, "scw", seq)
}

/// Snapshot every cookie in the writer jar as `(name, value)` pairs. Upload
/// payloads live in the values of `scu<seq>`, `scu<seq>-<i>`, and `scu<seq>-s`.
fn read_upload_cookies(window: &WebviewWindow) -> Option<Vec<(String, String)>> {
    let url: Url = HOME.parse().ok()?;
    let cookies = cookies_safe(window, url)?;
    Some(
        cookies
            .iter()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect(),
    )
}

/// `scu<seq>-<index>` chunk cookies, sorted by index. Non-numeric suffixes
/// (currently `-s`, the status cookie) are ignored.
fn upload_chunks(cookies: &[(String, String)], seq: u64) -> Vec<(usize, String)> {
    let prefix = format!("{UPLOAD_CHANNEL}{seq}-");
    let mut chunks: Vec<(usize, String)> = cookies
        .iter()
        .filter_map(|(name, value)| {
            let index = name.strip_prefix(&prefix)?.parse::<usize>().ok()?;
            Some((index, value.clone()))
        })
        .collect();
    chunks.sort_by_key(|(index, _)| *index);
    chunks
}

/// `scu<seq>-s`: the XHR status that produced the chunk cookies.
fn upload_status(cookies: &[(String, String)], seq: u64) -> Option<u16> {
    let name = format!("{UPLOAD_CHANNEL}{seq}-s");
    cookies
        .iter()
        .find(|(cookie, _)| cookie == &name)
        .and_then(|(_, value)| value.parse().ok())
}

/// Concatenate chunks in index order and URL-decode the result a single time.
fn join_upload_chunks(chunks: &[(usize, String)]) -> String {
    let mut sorted: Vec<&(usize, String)> = chunks.iter().collect();
    sorted.sort_by_key(|(index, _)| *index);
    let mut encoded = String::new();
    for (_, value) in sorted {
        encoded.push_str(value);
    }
    decode_channel_value(&encoded)
}

/// A `DONE:<total>` marker only completes once every index `0..total` exists.
fn chunks_complete(total: usize, chunks: &[(usize, String)]) -> bool {
    (0..total).all(|index| chunks.iter().any(|(chunk, _)| *chunk == index))
}

/// Expire every `scu*` cookie in the writer jar (`scu<seq>`, `scu<seq>-<i>`,
/// `scu<seq>-s`). Called before a start and after every outcome so chunk
/// cookies from one upload never leak into the next.
fn clear_upload_cookies(window: &WebviewWindow) {
    let Some(url) = HOME.parse::<Url>().ok() else {
        return;
    };
    let Some(cookies) = cookies_safe(window, url) else {
        return;
    };
    let mut js = String::new();
    for cookie in cookies
        .iter()
        .filter(|c| c.name().starts_with(UPLOAD_CHANNEL))
    {
        let Ok(name) = serde_json::to_string(cookie.name()) else {
            continue;
        };
        js.push_str(&format!("document.cookie={name}+'=;Max-Age=0;path=/';"));
    }
    if !js.is_empty() {
        let _ = eval(window, &js);
    }
}

async fn wait_ready(window: &WebviewWindow) -> Result<(), String> {
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
async fn prepared_window(app: &AppHandle, token: Option<&str>) -> Result<WebviewWindow, String> {
    let window = writer_window(app).ok_or_else(|| "writer window unavailable".to_string())?;
    wait_ready(&window).await?;
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        inject_session(&window, t);
    }
    Ok(window)
}

async fn do_fetch(
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
/// lane mutex, so it must not overlap other writer traffic.
pub async fn page_fetch_probe(app: &AppHandle, url: &str) -> Result<Value, String> {
    let _lane = LANE.lock().await;
    let window = prepared_window(app, None).await?;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let url_js = serde_json::to_string(url).map_err(|e| e.to_string())?;
    let js = format!(
        r#"fetch({url_js}).then(async function(r){{
  var b = await r.blob();
  document.cookie='scw={seq}:'+r.status+':'+encodeURIComponent(JSON.stringify({{size:b.size,type:b.type}}))+';path=/';
}}).catch(function(e){{
  document.cookie='scw={seq}:ERR:'+encodeURIComponent(String(e&&e.message||e).slice(0,300))+';path=/';
}});"#
    );
    eval(&window, &js)?;

    let deadline = Instant::now() + FETCH_TIMEOUT;
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(&window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            if let Some(msg) = rest.strip_prefix("ERR:") {
                return Err(format!("page fetch probe: {msg}"));
            }
            let (status, encoded) = rest.split_once(':').unwrap_or((rest.as_str(), ""));
            let status: u16 = status.parse().map_err(|_| format!("bad result: {rest}"))?;
            let info: Value = urlencoding::decode(encoded)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null);
            return Ok(serde_json::json!({
                "status": status,
                "size": info.get("size").cloned().unwrap_or(Value::Null),
                "type": info.get("type").cloned().unwrap_or(Value::Null),
            }));
        }
        if Instant::now() >= deadline {
            return Err("page fetch probe timed out".into());
        }
    }
}

fn json_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

/// Build the one-shot JS that fetches the audio/artwork bytes and sends the
/// multipart POST. Every dynamic string is JSON encoded before it is embedded.
/// `with_credentials` opts the XHR into the page cookies (DataDome), which
/// requires the API's CORS to echo the origin and allow credentials.
fn build_upload_js(
    seq: u64,
    upload_id: &str,
    token: &str,
    client_id: &str,
    app_version: &str,
    api_base: &str,
    track_json: &str,
    audio_url: &str,
    audio_filename: &str,
    artwork_url: Option<&str>,
    with_credentials: bool,
) -> Result<String, String> {
    let upload_id_js = json_string(upload_id)?;
    let token_js = json_string(token)?;
    let client_id_js = json_string(client_id)?;
    let app_version_js = json_string(app_version)?;
    let api_base_js = json_string(api_base)?;
    let audio_url_js = json_string(audio_url)?;
    let audio_filename_js = json_string(audio_filename)?;
    let artwork_url_js = match artwork_url {
        Some(url) => json_string(url)?,
        None => "null".to_string(),
    };
    let credentials_js = if with_credentials { "true" } else { "false" };

    // The web uploader is a three-step flow: mint an S3 upload policy, PUT the
    // bytes to the signed URL, wait for transcoding, then create the track with
    // a JSON body that references the upload uid. The documented multipart form
    // is rejected by api-v2 ("Failed to parse track create request"), so the
    // writer page has to speak the web app's protocol.
    Ok(format!(
        r#"(function(){{
  var SEQ = {seq};
  var API = {api_base_js};
  var CID = {client_id_js};
  var AV = {app_version_js};
  var TOKEN = {token_js};
  var CRED = {credentials_js};
  var CH = {chunk_max};
  function cookie(name, value){{ document.cookie = name + '=' + value + ';path=/'; }}
  function ev(value){{ cookie('{UPLOAD_CHANNEL}' + SEQ, value); }}
  function fail(message){{ ev('ERR:' + encodeURIComponent(String(message || '').slice(0, 300))); }}
  function sleep(ms){{ return new Promise(function(resolve){{ setTimeout(resolve, ms); }}); }}
  function api(path){{ return API + path + (path.indexOf('?') >= 0 ? '&' : '?') + 'client_id=' + encodeURIComponent(CID); }}
  var ID = {upload_id_js};
  function cancelled(){{ return !!(window.__upCancel && window.__upCancel[ID]); }}
  function emitResult(text, status){{
    var enc = encodeURIComponent(String(text).slice(0, 8000));
    var n = Math.max(1, Math.ceil(enc.length / CH));
    for (var i = 0; i < n; i++) cookie('{UPLOAD_CHANNEL}' + SEQ + '-' + i, enc.slice(i * CH, (i + 1) * CH));
    cookie('{UPLOAD_CHANNEL}' + SEQ + '-s', String(status));
    ev('DONE:' + n);
  }}
  async function uploadArtwork(createdText){{
    var created = JSON.parse(createdText);
    var urn = created && created.urn;
    if (!urn) return;
    var response = await fetch({artwork_url_js});
    if (!response.ok) throw new Error('artwork fetch ' + response.status);
    var blob = await response.blob();
    var data = await new Promise(function(resolve){{
      var reader = new FileReader();
      reader.onloadend = function(){{ resolve(reader.result); }};
      reader.readAsDataURL(blob);
    }});
    // The web app sends the base64 payload only, without the data: prefix.
    var base64 = String(data).split(',')[1];
    var putResponse = await fetch(API + '/tracks/' + encodeURIComponent(urn) + '/artwork?client_id=' + encodeURIComponent(CID), {{
      method: 'PUT',
      credentials: CRED ? 'include' : 'omit',
      headers: {{ 'Authorization': 'OAuth ' + TOKEN, 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ image_data: base64 }})
    }});
    if (!putResponse.ok) throw new Error('artwork upload ' + putResponse.status + ' ' + String(await putResponse.text()).slice(0, 200));
  }}
  function withArtworkError(text, message){{
    try {{
      var parsed = JSON.parse(text);
      parsed.artwork_error = String(message || 'artwork upload failed').slice(0, 300);
      return JSON.stringify(parsed);
    }} catch (_e) {{ return text; }}
  }}
  async function main(){{
    ev('START');
    var audioResponse = await fetch({audio_url_js});
    if (!audioResponse.ok) throw new Error('audio fetch ' + audioResponse.status);
    var blob = await audioResponse.blob();
    ev('P:1');
    var policyResponse = await fetch(api('/uploads/track-upload-policy'), {{
      method: 'POST',
      credentials: CRED ? 'include' : 'omit',
      headers: {{ 'Authorization': 'OAuth ' + TOKEN, 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ filename: {audio_filename_js}, filesize: blob.size }})
    }});
    if (!policyResponse.ok) throw new Error('upload policy ' + policyResponse.status + ' ' + String(await policyResponse.text()).slice(0, 300));
    var policy = await policyResponse.json();
    if (!policy || !policy.url || !policy.uid) throw new Error('upload policy missing url/uid');
    await new Promise(function(resolve, reject){{
      var xhr = new XMLHttpRequest();
      window.__upXhrs = window.__upXhrs || {{}};
      window.__upXhrs[{upload_id_js}] = xhr;
      xhr.open(policy.method || 'PUT', policy.url);
      var headers = policy.headers || {{}};
      for (var name in headers) {{
        if (!/^content-length$/i.test(name) && !/^host$/i.test(name) && !/^expect$/i.test(name)) xhr.setRequestHeader(name, headers[name]);
      }}
      xhr.setRequestHeader('Content-Type', 'application/octet-stream');
      xhr.upload.onprogress = function(e){{
        if (e.lengthComputable) ev('P:' + (3 + Math.round(e.loaded / e.total * 82)));
      }};
      xhr.onload = function(){{
        delete window.__upXhrs[{upload_id_js}];
        if (xhr.status >= 200 && xhr.status < 300) resolve();
        else reject(new Error('audio upload ' + xhr.status + ' ' + String(xhr.responseText || '').slice(0, 300)));
      }};
      xhr.onerror = function(){{ delete window.__upXhrs[{upload_id_js}]; reject(new Error('audio upload network error')); }};
      xhr.onabort = function(){{ delete window.__upXhrs[{upload_id_js}]; ev('ABORTED'); reject(new Error('__aborted')); }};
      xhr.send(blob);
    }});
    if (cancelled()) {{ ev('ABORTED'); return; }}
    var triggered = false;
    var transcodingUrl = api('/uploads/' + encodeURIComponent(policy.uid) + '/track-transcoding');
    for (var attempt = 0; attempt < 5 && !triggered; attempt++) {{
      try {{
        var trigger = await fetch(transcodingUrl, {{
          method: 'POST',
          credentials: CRED ? 'include' : 'omit',
          headers: {{ 'Authorization': 'OAuth ' + TOKEN }}
        }});
        if (trigger.ok) triggered = true;
        else await sleep(800);
      }} catch (_e) {{ await sleep(800); }}
    }}
    if (!triggered) throw new Error('transcoding trigger failed');
    ev('P:86');
    var finished = false;
    for (var poll = 0; poll < 600; poll++) {{
      if (cancelled()) {{ ev('ABORTED'); return; }}
      var statusResponse = await fetch(transcodingUrl, {{
        method: 'GET',
        credentials: CRED ? 'include' : 'omit',
        headers: {{ 'Authorization': 'OAuth ' + TOKEN }}
      }});
      if (statusResponse.status === 500) {{ await sleep(1500); continue; }}
      if (!statusResponse.ok) throw new Error('transcoding status ' + statusResponse.status);
      var status = await statusResponse.json();
      if (status.status === 'finished') {{ finished = true; break; }}
      if (status.status === 'failure' || status.status === 'not_found') throw new Error('transcoding ' + status.status);
      if (status.status === 'transcoding' && typeof status.percentage === 'number') ev('P:' + (86 + Math.round(status.percentage * 12 / 100)));
      await sleep(1500);
    }}
    if (!finished) throw new Error('transcoding timed out');
    if (cancelled()) {{ ev('ABORTED'); return; }}
    ev('P:99');
    var body = {track_json};
    body.uid = policy.uid;
    body.original_filename = {audio_filename_js};
    var createResponse = await fetch(api('/tracks') + '&app_version=' + encodeURIComponent(AV), {{
      method: 'POST',
      credentials: CRED ? 'include' : 'omit',
      headers: {{ 'Authorization': 'OAuth ' + TOKEN, 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ track: body }})
    }});
    var text = await createResponse.text();
    if (createResponse.ok && {artwork_url_js}) {{
      try {{ await uploadArtwork(text); }}
      catch (artworkError) {{ text = withArtworkError(text, artworkError && artworkError.message); }}
    }}
    emitResult(text, createResponse.status);
  }}
  main().catch(function(e){{ if (cancelled()) {{ ev('ABORTED'); return; }} fail(e && e.message || e); }});
}})();"#,
        chunk_max = UPLOAD_CHUNK_MAX,
    ))
}

/// PUT/POST a `FormData` body from the writer page. Used by the sharing
/// update and the track-edit diagnostic: SoundCloud's track update parser
/// expects form fields (`track[sharing]`), not the JSON envelope the create
/// endpoint takes.
pub async fn execute_form(
    app: &AppHandle,
    token: &str,
    method: &str,
    url: &str,
    fields: &[(String, String)],
) -> Result<(u16, String), String> {
    let _lane = LANE.lock().await;
    let window = prepared_window(app, Some(token)).await?;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let url_js = json_string(url)?;
    let token_js = json_string(token)?;
    let method_js = json_string(method)?;
    let mut fields_js = String::new();
    for (name, value) in fields {
        fields_js.push_str(&format!(
            "fd.append({}, {});\n",
            json_string(name)?,
            json_string(value)?
        ));
    }
    let js = format!(
        r#"var fd = new FormData();
{fields_js}fetch({url_js}, {{method:{method_js}, credentials:'include', headers:{{'Authorization':'OAuth '+{token_js}}}, body:fd}})
  .then(async function(r){{ var t=await r.text(); document.cookie='scw={seq}:'+r.status+':'+encodeURIComponent(String(t).slice(0,800))+';path=/'; }})
  .catch(function(e){{ document.cookie='scw={seq}:ERR:'+encodeURIComponent(String(e&&e.message||e).slice(0,200))+';path=/'; }});"#
    );
    eval(&window, &js)?;

    let deadline = Instant::now() + FETCH_TIMEOUT;
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(&window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            if let Some(msg) = rest.strip_prefix("ERR:") {
                return Err(format!("page form: {msg}"));
            }
            let (status, encoded) = rest.split_once(':').unwrap_or((rest.as_str(), ""));
            let status: u16 = status.parse().map_err(|_| format!("bad result: {rest}"))?;
            let payload = urlencoding::decode(encoded)
                .map(|s| s.into_owned())
                .unwrap_or_default();
            return Ok((status, payload));
        }
        if Instant::now() >= deadline {
            return Err("form fetch timed out".into());
        }
    }
}

/// `None` while the upload is within limits, otherwise the reason to abort.
/// No stall is reported before the first `Progress`: the audio fetch happens
/// before `xhr.send`, so a large file can stay silent for a long time. The
/// overall deadline still applies from the call.
fn stall_reason(
    last_progress: Option<Instant>,
    now: Instant,
    started: Instant,
) -> Option<&'static str> {
    if now.duration_since(started) >= UPLOAD_TIMEOUT {
        return Some("upload timed out");
    }
    let last = last_progress?;
    if now.duration_since(last) >= UPLOAD_STALL {
        return Some("upload stalled");
    }
    None
}

/// `None` while an event has been observed or the start window is still open.
/// A channel that never carries an event (for example, because the browser
/// rejected every cookie name) fails here instead of at `UPLOAD_TIMEOUT`.
fn start_timeout_reason(
    seen_event: bool,
    now: Instant,
    started: Instant,
) -> Option<&'static str> {
    if seen_event || now.duration_since(started) < UPLOAD_START_TIMEOUT {
        return None;
    }
    Some("the writer page never started the upload")
}

/// Send a track upload from the writer page and wait for its result. The page
/// runs the web uploader's protocol: S3 policy, signed PUT, transcoding poll,
/// then a JSON `POST /tracks`. Other writer traffic can keep using `scw`.
pub async fn do_upload(
    app: &AppHandle,
    upload_id: &str,
    token: &str,
    client_id: &str,
    app_version: &str,
    api_base: &str,
    track_json: &str,
    audio_url: &str,
    audio_filename: &str,
    artwork_url: Option<&str>,
    with_credentials: bool,
) -> Result<UploadOutcome, String> {
    let _upload_lane = UPLOAD_LANE.lock().await;
    let window = {
        let _lane = LANE.lock().await;
        prepared_window(app, Some(token)).await?
    };

    // Expire leftover `scu*` cookies from an earlier upload so its chunks
    // cannot be mistaken for ours.
    clear_upload_cookies(&window);

    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let js = build_upload_js(
        seq,
        upload_id,
        token,
        client_id,
        app_version,
        api_base,
        track_json,
        audio_url,
        audio_filename,
        artwork_url,
        with_credentials,
    )?;
    eval(&window, &js)?;

    let started = Instant::now();
    let mut last_progress: Option<Instant> = None;
    let mut last_percent: Option<u8> = None;
    let mut done_total: Option<usize> = None;
    let mut seen_event = false;
    let base_name = format!("{UPLOAD_CHANNEL}{seq}");
    loop {
        tokio::time::sleep(POLL).await;
        let now = Instant::now();
        if let Some(cookies) = read_upload_cookies(&window) {
            let base = cookies
                .iter()
                .find(|(name, _)| name == &base_name)
                .map(|(_, value)| value.clone());
            if let Some(value) = base {
                let event = parse_upload_event(&value);
                if event.is_some() {
                    seen_event = true;
                }
                match event {
                    // The audio fetch finished and `xhr.send` is next; waiting
                    // for the first progress event stays allowed.
                    Some(UploadEvent::Started) => {}
                    Some(UploadEvent::Progress(percent)) => {
                        if last_percent != Some(percent) {
                            last_percent = Some(percent);
                            last_progress = Some(now);
                            let _ = app.emit(
                                UPLOAD_PROGRESS_EVENT,
                                serde_json::json!({ "uploadId": upload_id, "percent": percent }),
                            );
                        }
                    }
                    Some(UploadEvent::Chunks { total }) => done_total = Some(total),
                    Some(UploadEvent::Done { status, payload }) => {
                        clear_upload_cookies(&window);
                        return Ok(UploadOutcome { status, payload });
                    }
                    Some(UploadEvent::Failed(message)) => {
                        clear_upload_cookies(&window);
                        return Err(message);
                    }
                    Some(UploadEvent::Aborted) => {
                        clear_upload_cookies(&window);
                        return Err("cancelled".into());
                    }
                    None => {}
                }
            }
            if let Some(total) = done_total {
                let chunks = upload_chunks(&cookies, seq);
                if chunks_complete(total, &chunks) {
                    let payload = join_upload_chunks(&chunks);
                    let status = upload_status(&cookies, seq).unwrap_or(0);
                    clear_upload_cookies(&window);
                    return Ok(UploadOutcome { status, payload });
                }
            }
        }
        if let Some(reason) = start_timeout_reason(seen_event, now, started) {
            clear_upload_cookies(&window);
            return Err(reason.into());
        }
        if let Some(reason) = stall_reason(last_progress, now, started) {
            clear_upload_cookies(&window);
            return Err(reason.into());
        }
    }
}

/// Abort the XHR kept by the writer page for `upload_id`.
pub fn abort_upload(app: &AppHandle, upload_id: &str) {
    let Some(window) = current_writer_window(app) else {
        return;
    };
    let Ok(upload_id_js) = serde_json::to_string(upload_id) else {
        return;
    };
    let js = format!(
        "try{{window.__upCancel=window.__upCancel||{{}};window.__upCancel[{upload_id_js}]=true;var x=window.__upXhrs&&window.__upXhrs[{upload_id_js}];if(x){{x.__upAborted=true;x.abort();}}}}catch(_e){{}}"
    );
    let _ = eval(&window, &js);
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
async fn wait_for_page(window: &WebviewWindow, needle: &str, timeout: Duration) -> Result<(), String> {
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
const SNIFF_HOOK_JS: &str = r#"try{
  if(!window.__sniffed)window.__sniffed=[];
  if(!window.__sniffWrapped){
    window.__sniffWrapped=true;
    try{
      window.__snifferrs=[];
      window.onerror=function(m){try{if(window.__snifferrs.length<10)window.__snifferrs.push(String(m).slice(0,150));}catch(_e){}};
    }catch(_e){}
    try{
      var _p=HTMLMediaElement.prototype.play;
      HTMLMediaElement.prototype.play=function(){try{
        if(!window.__sniffed)window.__sniffed=[];
        window.__sniffed.push('PLAYCALLED muted='+this.muted+' src='+(this.currentSrc||'').slice(-60));
        this.muted=true;
      }catch(_e){}
      try{return _p.call(this).then(function(){try{window.__sniffed.push('PLAYOK');}catch(_e){}},function(err){try{window.__sniffed.push('PLAYFAIL '+(err&&err.name||err));}catch(_e){}});}catch(e){try{return _p.call(this);}catch(_e){}}};
      window.__sniffed.push('MUTEHOOK installed');
    }catch(_e){window.__sniffed.push('MUTEHOOK fail');}
    try{
      var _r=AudioContext.prototype.resume;
      AudioContext.prototype.resume=function(){try{
        if(!window.__sniffed)window.__sniffed=[];
        window.__sniffed.push('ACRESUME state='+this.state);
      }catch(_e){}return _r.call(this);};
    }catch(_e){}
    var keep=function(m,s){return m!=='GET'||/media\/|stream|play-history|play_history|listen|scrobble/i.test(s);};
    var rec=function(m,s,b){try{
      var bs=String(b||'');
      var lim=(s.indexOf('/me?')>=0)?1500:400;
      window.__sniffed.push(m+' '+s.slice(0,240)+' BODY:'+bs.slice(0,lim));
      if(window.__sniffed.length>60)window.__sniffed.shift();
    }catch(_e){}};
    var recBatch=function(m,s,b){try{
      var hits=[];
      try{
        var j=JSON.parse(String(b||''));
        var arrs=[];
        if(j.batch&&j.batch.length)arrs.push(j.batch);
        if(j.events&&j.events.length)arrs.push(j.events);
        for(var a=0;a<arrs.length;a++){for(var e=0;e<arrs[a].length;e++){
          var es=JSON.stringify(arrs[a][e]);
          if(/play|audio|checkpoint|listen/i.test(es))hits.push(es.slice(0,2000));
        }}
        // Keep the complete raw body for exact replay experiments.
        if(hits.length){try{window.__fullbatch=String(b);}catch(_e){}}
      }catch(_e){}
      window.__sniffed.push(m+' '+s.slice(0,160)+' BATCHPLAY total='+hits.length+' :: '+hits.join(' || ').slice(0,2400));
      if(window.__sniffed.length>60)window.__sniffed.shift();
    }catch(_e){}};
    var _f=window.fetch;
    window.fetch=function(u,o){try{
      var m=((o&&o.method)||'GET').toUpperCase();
      var s=String((u&&u.url)||u);
      var isApi=s.indexOf('api-v2.soundcloud.com')>=0;
      if(m!=='GET'){
        if(isApi&&(s.indexOf('/me?')>=0||s.indexOf('play-history')>=0)){
          try{window.__sniffed.push('HDRS fetch '+s.slice(0,120)+' :: '+JSON.stringify((o&&o.headers)||{}).slice(0,400));}catch(_e){}
        }
        if(isApi&&s.indexOf('/me?')>=0){recBatch(m,s,(o&&o.body)||'');}
        else{rec(m+'@'+(isApi?'api':'ext'),s,(o&&o.body)||'');}
      }else if(isApi&&keep(m,s)){rec(m,s,(o&&o.body)||'');}
    }catch(_e){}
    return _f.apply(this,arguments);};
    var _o=XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open=function(m,u){try{
      this.__m=String(m||'GET').toUpperCase();this.__u=String(u||'');this.__h={};
    }catch(_e){}return _o.apply(this,arguments);};
    var _sh=XMLHttpRequest.prototype.setRequestHeader;
    XMLHttpRequest.prototype.setRequestHeader=function(k,v){try{
      if(!this.__h)this.__h={};this.__h[String(k)]=String(v).slice(0,120);
    }catch(_e){}return _sh.apply(this,arguments);};
    var _s=XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send=function(b){try{
      var u=String(this.__u||'');
      var isApi=u.indexOf('api-v2.soundcloud.com')>=0;
      var mm=this.__m||'?';
      if(mm!=='GET'){
        if(isApi&&(u.indexOf('/me?')>=0||u.indexOf('play-history')>=0)){
          try{window.__sniffed.push('HDRS '+u.slice(0,120)+' :: '+JSON.stringify(this.__h||{}).slice(0,400));}catch(_e){}
        }
        if(isApi&&u.indexOf('/me?')>=0){recBatch('XHR '+mm,u,b);}
        else{rec('XHR '+mm+'@'+(isApi?'api':'ext'),u,b);}
      }else if(isApi&&keep(mm,u)){rec('XHR '+mm,u,b);}
    }catch(_e){}return _s.apply(this,arguments);};
  }
}catch(_e){}"#;

/// Drain `window.__sniffed` through the cookie channel (best-effort).
/// Returns the entries, or a single marker when the hook context is gone.
async fn read_sniff(window: &WebviewWindow) -> Vec<String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let _ = eval(
        window,
        &format!(
            r#"try{{if(window.__sniffed&&window.__sniffed.length){{document.cookie='scw={seq}:'+encodeURIComponent(JSON.stringify(window.__sniffed).slice(0,3000))+';path=/';window.__sniffed=[];}}else{{document.cookie='scw={seq}:EMPTY:'+encodeURIComponent(String(window.__sniffWrapped||'nowrap')+'@'+location.href.slice(0,60))+';path=/';}}}}catch(_e){{}}"#
        ),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            if let Some(marker) = rest.strip_prefix("EMPTY:") {
                return vec![format!("(drain: {marker})")];
            }
            let payload = urlencoding::decode(&rest)
                .map(|s| s.into_owned())
                .unwrap_or_default();
            return serde_json::from_str(&payload).unwrap_or_else(|_| vec![payload]);
        }
        if Instant::now() >= deadline {
            return vec!["(drain: timeout)".into()];
        }
    }
}

/// JS that finds play buttons and clicks the first visible one.
const SNIFF_CLICK_JS: &str = r#"try{
  if(!window.__sniffed)window.__sniffed=[];
  try{
    var closers=['#onetrust-accept-btn-handler','button[aria-label="Close"]','.modal__closeButton','buttononetrust-close-btn-handler'];
    for(var c=0;c<closers.length;c++){var cb2=document.querySelector(closers[c]);if(cb2&&cb2.offsetParent!==null){cb2.click();window.__sniffed.push('CLOSED '+closers[c]);break;}}
  }catch(_e){}
  var sels=['.soundTitle__playButton','button.sc-button-play','.playControls__play','button[aria-label="Play"]','button[title="Play"]','[data-testid="play-button"]'];
  var hit='';
  for(var i=0;i<sels.length;i++){var b=document.querySelector(sels[i]);if(b&&b.offsetParent!==null){b.click();hit=sels[i];break;}}
  if(!hit){var b2=document.querySelector('.playControls__play');if(b2){b2.click();hit='.playControls__play(hidden)';}}
  window.__sniffed.push(hit?('CLICK '+hit):'NOBUTTON url='+location.href.slice(0,120));
}catch(e){try{if(!window.__sniffed)window.__sniffed=[];window.__sniffed.push('CLICKERR '+String(e&&e.message||e).slice(0,120));}catch(_){}}"#;

/// Read back the complete raw `/me` batch body captured by the sniffer,
/// in 2000-char cookie chunks (the return path has no size limit).
async fn read_fullbatch(window: &WebviewWindow) -> Option<String> {
    let mut out = String::new();
    for i in 0..8usize {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let _ = eval(
            window,
            &format!(
                "try{{var fb=String(window.__fullbatch||'');var ch=fb.slice({i}*2000,({i}+1)*2000);document.cookie='scw={seq}:FB:'+ch.length+':'+encodeURIComponent(ch)+';path=/';}}catch(_e){{}}"
            ),
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut done = true;
        loop {
            tokio::time::sleep(POLL).await;
            if let Some(rest) = read_channel(window, seq) {
                let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
                if let Some(r) = rest.strip_prefix("FB:") {
                    if let Some((len, data)) = r.split_once(':') {
                        if len != "0" {
                            let chunk = urlencoding::decode(data)
                                .map(|s| s.into_owned())
                                .unwrap_or_default();
                            let last = chunk.len() < 2000;
                            out.push_str(&chunk);
                            done = last;
                        }
                    }
                }
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        if done {
            break;
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// TEMP-DIAG(history): end-to-end hot replay test. Drives official playback
/// until a real audio checkpoint is captured, then immediately re-POSTs the
/// identical body (fresh ts/sent_at only, same datadome header) in the same
/// live session. Answers whether acceptance is about session-hotness.
pub async fn hot_replay(app: &AppHandle, token: &str, page_url: &str) -> Result<Value, String> {
    let _lane = LANE.lock().await;
    let window = writer_window(app).ok_or_else(|| "writer unavailable".to_string())?;
    inject_session(&window, token);
    let url_js = serde_json::to_string(page_url).map_err(|e| e.to_string())?;
    let _ = eval(&window, &format!("location.href={url_js}"));
    let slug = page_url.rsplit('/').next().unwrap_or("soundcloud.com");
    let _ = wait_for_page(&window, slug, Duration::from_secs(30)).await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    let _ = eval(&window, SNIFF_HOOK_JS);
    if let Err(e) = eval(&window, SNIFF_CLICK_JS) {
        return Err(format!("click failed: {e}"));
    }
    // Phase 1: wait for real playback (PLAYOK) and capture one checkpoint.
    // The paired HDRS line carries the live x-datadome-clientid header.
    let mut checkpoint: Option<String> = None;
    let mut datadome: Option<String> = None;
    let mut playing = false;
    let mut acc: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline && checkpoint.is_none() {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let _ = eval(&window, SNIFF_HOOK_JS);
        if !playing {
            let _ = eval(&window, SNIFF_CLICK_JS);
        }
        for it in read_sniff(&window).await {
            if it.contains("PLAYOK") {
                playing = true;
            } else if it.contains("PLAYFAIL") {
                playing = false;
            }
            if datadome.is_none() && it.starts_with("HDRS ") && it.contains("/me?") {
                if let Some(js) = it.split(" :: ").nth(1) {
                    if let Ok(h) = serde_json::from_str::<Value>(js) {
                        if let Some(id) = h.get("x-datadome-clientid").and_then(Value::as_str) {
                            datadome = Some(id.to_string());
                        }
                    }
                }
            }
            acc.push(it);
        }
        // Pull the full raw batch; exit as soon as a checkpoint is captured.
        // (No PLAYOK gate: the player streams via WebAudio, so <audio>
        // play() hooks never fire. Footer aria would tell, but the
        // checkpoint itself is the ground truth.)
        if let Some(raw) = read_fullbatch(&window).await {
            if raw.contains("\"action\":\"checkpoint\"") {
                checkpoint = Some(raw);
            }
        }
    }
    let raw = checkpoint.ok_or_else(|| {
        let tail: Vec<&str> = acc.iter().rev().take(6).map(String::as_str).collect();
        format!("no live checkpoint captured; recent: {}", tail.join(" // ").chars().take(600).collect::<String>())
    })?;
    // Phase 2: patch ts/sent_at, re-POST in the same hot session.
    let mut body: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let now_iso = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    if let Some(evts) = body.get_mut("events").and_then(Value::as_array_mut) {
        for e in evts.iter_mut() {
            if let Some(p) = e.get_mut("payload") {
                if let Some(obj) = p.as_object_mut() {
                    obj.insert("ts".into(), Value::from(now_ms));
                }
            }
        }
    }
    if let Some(obj) = body.as_object_mut() {
        obj.insert("sent_at".into(), Value::from(now_iso.clone()));
    }
    // Datadome header: the live tab's id captured alongside, else cookie.
    if datadome.is_none() {
        datadome = read_datadome_id(&window).await;
    }
    let mut extra = serde_json::Map::new();
    if let Some(id) = datadome {
        extra.insert("x-datadome-clientid".into(), Value::from(id));
    }
    let cid = read_client_id(&window).await.unwrap_or_default();
    let url = format!("https://api-v2.soundcloud.com/me?client_id={cid}");
    let (status, payload) =
        do_fetch(&window, None, &url, "POST", Some(&body), None, Some("application/json"), Some(&extra)).await?;
    Ok(serde_json::json!({ "replay_status": status, "replay_body": payload.chars().take(200).collect::<String>(), "sent_ts": now_ms }))
}

/// Read the live tab's DataDome client id (set by the sensor JS).
async fn read_datadome_id(window: &WebviewWindow) -> Option<String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let _ = eval(window, &format!("try{{var m=document.cookie.match(/datadome=([^;]+)/);document.cookie='scw={seq}:DD:'+encodeURIComponent(m?m[1]:'')+';path=/';}}catch(_e){{}}"));
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            let id = rest.strip_prefix("DD:").unwrap_or("").to_string();
            let id = urlencoding::decode(&id).map(|s| s.into_owned()).unwrap_or_default();
            return if id.is_empty() { None } else { Some(id) };
        }
        if Instant::now() >= deadline {
            return None;
        }
    }
}

/// Read the live tab's api-v2 client_id (page URL may carry a fresh one).
async fn read_client_id(window: &WebviewWindow) -> Option<String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let _ = eval(window, &format!("try{{var m=(document.documentElement.innerHTML||'').match(/client_id=([A-Za-z0-9]+)/);document.cookie='scw={seq}:CI:'+encodeURIComponent(m?m[1]:'')+';path=/';}}catch(_e){{}}"));
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            let id = rest.strip_prefix("CI:").unwrap_or("").to_string();
            let id = urlencoding::decode(&id).map(|s| s.into_owned()).unwrap_or_default();
            return if id.is_empty() { None } else { Some(id) };
        }
        if Instant::now() >= deadline {
            return None;
        }
    }
}

/// TEMP-DIAG(history): drive the official web player in the writer page and
/// capture the api-v2 requests it emits around a play press. Returns the
/// captured log. Best-effort automation; never changes account state
/// beyond the single play itself.
pub async fn sniff_official_play(
    app: &AppHandle,
    token: &str,
    page_url: &str,
    wait_secs: u64,
) -> Result<Value, String> {
    let _lane = LANE.lock().await;
    let window = ensure_window(app).ok_or_else(|| "writer unavailable".to_string())?;
    inject_session(&window, token);
    let url_js = serde_json::to_string(page_url).map_err(|e| e.to_string())?;
    let _ = eval(&window, &format!("location.href={url_js}"));
    // Sync on the loaded track page (SPA navigation keeps context; full load
    // wipes it 窶・either way the hook below installs into the live document).
    let slug = page_url.rsplit('/').next().unwrap_or("soundcloud.com");
    let _ = wait_for_page(&window, slug, Duration::from_secs(30)).await;
    // SPA keeps mounting after document.complete (player chrome, waveform);
    // settle before hooking so the first context isn't wiped mid-install.
    tokio::time::sleep(Duration::from_secs(6)).await;

    // Install the traffic hook (idempotent per document; safe to re-run after
    // re-navigation since the array init no longer wipes).
    let mut acc: Vec<String> = Vec::new();
    if let Err(e) = eval(&window, SNIFF_HOOK_JS) {
        acc.push(format!("(hook eval err init: {e})"));
    }
    // Header (track) play button first: it queues the track into the player.
    // Footer control second.
    if let Err(e) = eval(&window, SNIFF_CLICK_JS) {
        acc.push(format!("(click eval err init: {e})"));
    }

    // Accumulate incrementally: re-hook (covers re-navigation) and drain the
    // array every few seconds so a later context wipe can't lose everything.
    // State-aware clicking: PLAYOK means the media is (muted-)playing, so
    // further clicks would only toggle pause. Re-click solely after PLAYFAIL
    // or while no play attempt has been observed yet.
    let mut playing = false;
    let polls = (wait_secs / 5).max(1);
    for i in 0..polls {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if let Err(e) = eval(&window, SNIFF_HOOK_JS) {
            acc.push(format!("(hook eval err p{i}: {e})"));
        }
        if i > 0 && !playing {
            if let Err(e) = eval(&window, SNIFF_CLICK_JS) {
                acc.push(format!("(click eval err p{i}: {e})"));
            }
        }
        let items = read_sniff(&window).await;
        for it in &items {
            if it.contains("PLAYOK") {
                playing = true;
            } else if it.contains("PLAYFAIL") {
                playing = false;
            }
        }
        acc.extend(items);
    }

    let _ = eval(
        &window,
        r#"try{
      if(!window.__sniffed)window.__sniffed=[];
      var st='TITLE:'+document.title.slice(0,80);
      try{var md=document.querySelector('audio,video');st+=' MEDIA:'+(md?(md.paused?'paused':'playing'):'none');}catch(_e){}
      try{st+=' PLAYBTN:'+document.querySelectorAll('.playControls__play').length;}catch(_e){}
      try{var pb=document.querySelector('.playControls__play');st+=' FBARIA:'+((pb&&(pb.getAttribute('aria-label')||''))||'none');}catch(_e){}
      try{st+=' DIALOGS:'+document.querySelectorAll('[role="dialog"],.modalModal').length;}catch(_e){}
      try{var dg=document.querySelector('[role="dialog"],.modalModal');st+=' DHTML:'+((dg&&(dg.outerHTML||'')).slice(0,200));}catch(_e){}
      try{var cb=document.querySelector('.soundTitle__playButton');var r=cb?cb.getBoundingClientRect():null;var el=r?document.elementFromPoint(r.left+r.width/2,r.top+r.height/2):null;st+=' HIT:'+((el&&(el.tagName+'.'+(el.className&&el.className.baseVal!==undefined?el.className.baseVal:String(el.className||'')).slice(0,40)))||'none');}catch(_e){}
      try{var dcap=[document.querySelector('#datadome-captcha'),document.querySelector('#captcha')].filter(function(x){return !!x;}).length;st+=' DOME:'+dcap;}catch(_e){}
      try{st+=' ERRS:'+(window.__snifferrs||[]).join('|').slice(0,300);}catch(_e){}
      window.__sniffed.push(st);
    }catch(_e){}"#,
    );
    acc.extend(read_sniff(&window).await);
    if acc.is_empty() {
        acc.push("EMPTY: page context never held the hook".into());
    }
    let fullbatch = read_fullbatch(&window).await;
    let _ = eval(&window, &format!("location.href='{HOME}'"));
    Ok(serde_json::json!({ "log": acc, "fullbatch": fullbatch }))
}

/// TEMP-DIAG(history): resolve the HLS stream and fetch two segments inside
/// the writer page (real cookies/TLS fingerprint + OAuth) to test whether
/// server-side streaming attribution records play history.
pub async fn attr_play(
    app: &AppHandle,
    token: &str,
    track_id: &str,
    cid: &str,
) -> Result<Value, String> {
    let _lane = LANE.lock().await;
    let window = writer_window(app).ok_or_else(|| "writer unavailable".to_string())?;
    wait_ready(&window).await?;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let track_js = serde_json::to_string(track_id).map_err(|e| e.to_string())?;
    let token_js = serde_json::to_string(token).map_err(|e| e.to_string())?;
    let cid_js = serde_json::to_string(cid).map_err(|e| e.to_string())?;
    // NOTE: plain template + token replace (no format!: JS braces would need
    // error-prone manual doubling).
    const ATTR_TMPL: &str = r#"fetch("https://api-v2.soundcloud.com/tracks/"+@TRACK@+"?client_id="+@CID@,{headers:{"Authorization":"OAuth "+@TOKEN@}}).then(async function(r){
        var meta=await r.json().catch(function(){return {};});
        var trs=(meta&&meta.media&&meta.media.transcodings)||[];
        var tu="";
        for(var k=0;k<trs.length;k++){if(trs[k].format&&trs[k].format.protocol==="hls"){tu=trs[k].url;break;}}
        if(!tu&&trs.length)tu=trs[0].url;
        if(!tu){document.cookie='scw=@SEQ@:ERR:notranscode:'+r.status+';path=/';return;}
        var rep=await (await fetch(tu+"?client_id="+@CID@,{headers:{"Authorization":"OAuth "+@TOKEN@}})).json().catch(function(){return {};});
        var pu=(rep&&rep.url)||"";
        if(!pu){document.cookie='scw=@SEQ@:ERR:nourl:'+r.status+';path=/';return;}
        var pl=await (await fetch(pu,{headers:{"Authorization":"OAuth "+@TOKEN@}})).text();
        var segs=pl.split("\n").filter(function(l){return l&&l[0]!=='#';}).slice(0,2);
        var got=[];
        for(var i=0;i<segs.length;i++){
            try{
                var s=await fetch(segs[i]);
                var b=await s.arrayBuffer().catch(function(){return null;});
                got.push(s.status+':'+(b?b.byteLength:0));
            }catch(e){got.push('ERR');}
        }
        document.cookie='scw=@SEQ@:OK:'+encodeURIComponent(segs.length+' segs '+got.join(','))+';path=/';
    }).catch(function(e){document.cookie='scw=@SEQ@:ERR:'+encodeURIComponent(String(e&&e.message||e).slice(0,120))+';path=/';});"#;
    let js = ATTR_TMPL
        .replace("@TRACK@", &track_js)
        .replace("@CID@", &cid_js)
        .replace("@TOKEN@", &token_js)
        .replace("@SEQ@", &seq.to_string());
    eval(&window, &js)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        tokio::time::sleep(POLL).await;
        if let Some(rest) = read_channel(&window, seq) {
            let _ = window.eval("document.cookie='scw=;Max-Age=0;path=/';");
            if let Some(msg) = rest.strip_prefix("ERR:") {
                return Err(format!("attr-play: {msg}"));
            }
            let info = rest.strip_prefix("OK:").unwrap_or(&rest);
            let info = urlencoding::decode(info).map(|s| s.into_owned()).unwrap_or_default();
            return Ok(serde_json::json!({ "fetch": info }));
        }
        if Instant::now() >= deadline {
            return Err("attr-play timed out".into());
        }
    }
}

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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::{
        UPLOAD_CHUNK_MAX, UPLOAD_STALL, UPLOAD_START_TIMEOUT, UPLOAD_TIMEOUT, UploadEvent,
        build_upload_js, chunks_complete, join_upload_chunks, parse_upload_event,
        stall_reason, start_timeout_reason, upload_chunks, upload_status,
    };

    #[test]
    fn parses_progress() {
        assert_eq!(parse_upload_event("P:42"), Some(UploadEvent::Progress(42)));
    }

    #[test]
    fn parses_start() {
        assert_eq!(parse_upload_event("START"), Some(UploadEvent::Started));
    }

    #[test]
    fn parses_done_marker_as_chunks() {
        assert_eq!(
            parse_upload_event("DONE:2"),
            Some(UploadEvent::Chunks { total: 2 })
        );
        assert_eq!(
            parse_upload_event("DONE:0"),
            Some(UploadEvent::Chunks { total: 0 })
        );
        assert_eq!(parse_upload_event("DONE:two"), None);
        assert_eq!(parse_upload_event("DONE:"), None);
    }

    #[test]
    fn parses_done_with_encoded_payload() {
        assert_eq!(
            parse_upload_event("201:%7B%22id%22%3A1%7D"),
            Some(UploadEvent::Done {
                status: 201,
                payload: r#"{"id":1}"#.into(),
            })
        );
    }

    #[test]
    fn parses_failed_with_encoded_message() {
        assert_eq!(
            parse_upload_event("ERR:network%20down"),
            Some(UploadEvent::Failed("network down".into()))
        );
    }

    #[test]
    fn parses_aborted() {
        assert_eq!(parse_upload_event("ABORTED"), Some(UploadEvent::Aborted));
    }

    #[test]
    fn rejects_unknown_values() {
        assert_eq!(parse_upload_event("ready"), None);
        assert_eq!(parse_upload_event("P:not-a-number"), None);
        assert_eq!(parse_upload_event("P:300"), None);
        assert_eq!(parse_upload_event("nope:value"), None);
        assert_eq!(parse_upload_event(""), None);
    }

    #[test]
    fn status_without_payload_is_done() {
        assert_eq!(
            parse_upload_event("204"),
            Some(UploadEvent::Done {
                status: 204,
                payload: String::new(),
            })
        );
    }

    fn cookie(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    #[test]
    fn parses_chunks_in_index_order_and_joins_them() {
        let cookies = vec![cookie("scu12-1", "def"), cookie("scu12-0", "abc")];
        let chunks = upload_chunks(&cookies, 12);
        assert_eq!(
            chunks,
            vec![(0, "abc".to_string()), (1, "def".to_string())]
        );
        assert_eq!(join_upload_chunks(&chunks), "abcdef");
    }

    #[test]
    fn decodes_once_after_joining_chunks() {
        // The split can land inside a %XX escape, so a chunk must not be
        // decoded before the concatenation.
        let cookies = vec![cookie("scu12-0", "%7"), cookie("scu12-1", "B%22id%22%3A1%7D")];
        let chunks = upload_chunks(&cookies, 12);
        assert_eq!(join_upload_chunks(&chunks), r#"{"id":1}"#);
    }

    #[test]
    fn ignores_foreign_and_status_cookies() {
        let cookies = vec![
            cookie("scw42", "12:x"),
            cookie("scu12-s", "201"),
            cookie("scu123-0", "other"),
            cookie("scu12-0", "abc"),
        ];
        assert_eq!(upload_chunks(&cookies, 12), vec![(0, "abc".to_string())]);
        assert_eq!(upload_status(&cookies, 12), Some(201));
    }

    #[test]
    fn chunks_complete_needs_every_index() {
        assert!(!chunks_complete(2, &[(0, "abc".to_string())]));
        assert!(chunks_complete(
            2,
            &[(1, "def".to_string()), (0, "abc".to_string())]
        ));
        assert!(chunks_complete(0, &[]));
    }

    #[test]
    fn stall_reason_ignores_pre_send_wait() {
        let started = Instant::now();
        assert_eq!(stall_reason(None, started, started), None);
        assert_eq!(
            stall_reason(None, started + UPLOAD_STALL + Duration::from_secs(1), started),
            None
        );
    }

    #[test]
    fn stall_reason_fires_after_stall_without_progress() {
        let started = Instant::now();
        let last = started + Duration::from_secs(5);
        assert_eq!(
            stall_reason(
                Some(last),
                last + UPLOAD_STALL - Duration::from_secs(1),
                started
            ),
            None
        );
        assert_eq!(
            stall_reason(Some(last), last + UPLOAD_STALL, started),
            Some("upload stalled")
        );
    }

    #[test]
    fn stall_reason_keeps_overall_timeout_without_progress() {
        let started = Instant::now();
        assert_eq!(
            stall_reason(None, started + UPLOAD_TIMEOUT, started),
            Some("upload timed out")
        );
        // The overall deadline wins over a long-silent progress clock.
        let last = started + Duration::from_secs(10);
        assert_eq!(
            stall_reason(
                Some(last),
                started + UPLOAD_TIMEOUT + Duration::from_secs(60),
                started
            ),
            Some("upload timed out")
        );
    }

    fn build_track(track_json: &str) -> String {
        build_upload_js(
            7,
            "upload-1",
            "oauth-token",
            "client-id",
            "app-version",
            "https://api-v2.soundcloud.com",
            track_json,
            "http://127.0.0.1:4321/up/audio-token",
            "demo.mp3",
            None,
            true,
        )
        .expect("upload JS should build")
    }

    fn build_with(artwork_url: Option<&str>, with_credentials: bool) -> String {
        build_upload_js(
            7,
            "upload-1",
            "oauth-token",
            "client-id",
            "app-version",
            "https://api-v2.soundcloud.com",
            r#"{"title":"Demo","sharing":"private"}"#,
            "http://127.0.0.1:4321/up/audio-token",
            "demo.mp3",
            artwork_url,
            with_credentials,
        )
        .expect("upload JS should build")
    }

    fn build(artwork_url: Option<&str>) -> String {
        build_with(artwork_url, true)
    }

    #[test]
    fn upload_js_json_encodes_field_values() {
        let payload = r#""; document.cookie='x'; //"#;
        let track = serde_json::to_string(&json!({ "title": payload })).unwrap();
        let js = build_track(&track);
        assert!(js.contains(r#"var body = {"title":"\"; document.cookie='x'; //"};"#));
        assert!(!js.contains(r#"var body = {"title":""; document.cookie='x'; //"};"#));
    }

    #[test]
    fn upload_js_contains_the_web_upload_flow() {
        let js = build(None);
        assert!(js.contains("/uploads/track-upload-policy"));
        assert!(js.contains("/track-transcoding"));
        assert!(js.contains("'/tracks'"));
        assert!(js.contains("policy.uid"));
        assert!(js.contains("original_filename"));
        assert!(js.contains("JSON.stringify({ track: body })"));
        assert!(js.contains(r#""demo.mp3""#));
    }

    #[test]
    fn upload_js_adds_artwork_only_when_provided() {
        let without = build(None);
        assert!(!without.contains("/up/art-token"));

        let with = build(Some("http://127.0.0.1:4321/up/art-token"));
        assert!(with.contains("/up/art-token"));
        assert!(with.contains("/artwork?client_id="));
        assert!(with.contains("readAsDataURL"));
        // The API takes the base64 payload only, without the data: prefix.
        assert!(with.contains("split(',')[1]"));
        assert!(with.contains("image_data: base64"));
        assert!(with.contains("artwork_error"));
    }

    #[test]
    fn upload_js_splits_completion_into_chunk_cookies() {
        let js = build(None);
        assert!(js.contains(&format!("var CH = {UPLOAD_CHUNK_MAX};")));
        assert!(js.contains("cookie('scu' + SEQ + '-' + i"));
        assert!(js.contains("cookie('scu' + SEQ + '-s'"));
        assert!(js.contains("ev('DONE:' + n)"));
    }

    #[test]
    fn upload_js_marks_start_before_the_audio_fetch() {
        let js = build(None);
        let start = js.find("ev('START')").expect("START marker");
        let policy = js
            .find("/uploads/track-upload-policy")
            .expect("policy endpoint");
        assert!(start < policy);
    }

    #[test]
    fn upload_js_sets_with_credentials_only_when_requested() {
        assert!(build_with(None, false).contains("var CRED = false;"));
        assert!(build_with(None, true).contains("var CRED = true;"));
    }

    #[test]
    fn upload_js_cookie_names_are_tokens() {
        let js = build(None);
        assert!(js.contains("function cookie(name, value)"));
        assert!(js.contains("cookie('scu' + SEQ, value)"));
        assert!(js.contains("cookie('scu' + SEQ + '-' + i"));
        assert!(js.contains("cookie('scu' + SEQ + '-s'"));
        // No raw cookie assignment and no `:` in a cookie name.
        assert!(!js.contains("document.cookie='scu"));
        assert!(!js.contains("scu7:"));
    }

    #[test]
    fn upload_js_puts_event_kind_in_cookie_value() {
        let js = build(None);
        for literal in [
            "ev('P:'",
            "ev('START')",
            "ev('DONE:'",
            "ev('ERR:'",
            "ev('ABORTED')",
        ] {
            assert!(js.contains(literal), "missing `{literal}`");
        }
    }

    #[test]
    fn upload_js_checks_the_cancel_flag_between_steps() {
        let js = build(None);
        assert!(js.contains("function cancelled()"));
        assert!(js.contains("window.__upCancel"));
        assert!(js.contains("if (cancelled()) { ev('ABORTED'); return; }"));
    }

    #[test]
    fn start_timeout_fires_only_before_any_event() {
        let started = Instant::now();
        assert_eq!(
            start_timeout_reason(
                false,
                started + UPLOAD_START_TIMEOUT - Duration::from_secs(1),
                started
            ),
            None
        );
        assert_eq!(
            start_timeout_reason(false, started + UPLOAD_START_TIMEOUT, started),
            Some("the writer page never started the upload")
        );
        assert_eq!(
            start_timeout_reason(true, started + UPLOAD_START_TIMEOUT, started),
            None
        );
    }
}
