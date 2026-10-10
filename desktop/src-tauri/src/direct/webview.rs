//! Hidden WebView "writer" for SoundCloud mutations.
//!
//! DataDome scores the whole client — TLS/HTTP2 fingerprint plus its sensor
//! cookies — so writes sent by an HTTP client are challenged. A `fetch()` from
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
const HOME: &str = "https://soundcloud.com/";
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

fn writer_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(super::login::LABEL)
        .or_else(|| ensure_window(app))
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

fn read_channel(window: &WebviewWindow, seq: u64) -> Option<String> {
    let url: Url = HOME.parse().ok()?;
    let cookies = cookies_safe(window, url)?;
    let value = cookies
        .iter()
        .find(|c| c.name() == "scw")
        .map(|c| c.value().to_string())?;
    value.strip_prefix(&format!("{seq}:")).map(str::to_owned)
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
        let ct = content_type.unwrap_or("application/json");
        init["headers"]["Content-Type"] = serde_json::json!(ct);
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

fn is_challenge(status: u16, payload: &str) -> bool {
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

/// TEMP-DIAG(history): page fetch with an explicit Content-Type.
pub async fn execute_ct(
    app: &AppHandle,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&Value>,
    content_type: &str,
) -> Result<WriteOutcome, String> {
    let _lane = LANE.lock().await;
    let window = writer_window(app).ok_or_else(|| "writer window unavailable".to_string())?;
    wait_ready(&window).await?;
    let (status, payload) =
        do_fetch(&window, token, url, method, body, None, Some(content_type), None).await?;
    Ok(WriteOutcome { status, payload, captcha: None })
}

/* TEMP-DIAG(history): page fetch with explicit Content-Type, optional extra
   headers, and optional auth suppression (token None => no Authorization). */
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
    let window = writer_window(app).ok_or_else(|| "writer unavailable".to_string())?;
    wait_ready(&window).await?;
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
    let window = writer_window(app).ok_or_else(|| "writer window unavailable".to_string())?;
    wait_ready(&window).await?;

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
    // wipes it — either way the hook below installs into the live document).
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
