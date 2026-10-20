//! Track upload flow: one-time audio bridge, web uploader protocol, progress channel.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{Emitter, Url};

use crate::rt::{AppHandle, WebviewWindow};

use super::{
    HOME, LANE, POLL, SEQ, cookies_safe, current_writer_window, eval, json_string, prepared_window,
};

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
static UPLOAD_LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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
