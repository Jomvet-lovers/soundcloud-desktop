//! Writer-page diagnostics: binary fetch probe and form-data writes.

use std::sync::atomic::Ordering;
use std::time::Instant;

use serde_json::Value;

use crate::rt::AppHandle;

use super::{FETCH_TIMEOUT, LANE, POLL, SEQ, eval, json_string, prepared_window, read_channel};

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
