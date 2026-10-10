use serde_json::{json, Value};
use tauri::Manager;
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::sc::{SC_API, id_of};
use std::time::{Duration, Instant};

pub async fn route(ctx: &Ctx) -> Result<Response, warp::Rejection> {
    let s = &ctx.state;
    let q = &ctx.q;
    let token = &ctx.token;
    let body = &ctx.body;
    let p: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (ctx.method.as_str(), p.as_slice()) {
        ("GET", ["history"]) => {
            let limit = q_u64(&q, "limit", 50).clamp(1, 100);
            if let Some(t) = token.as_deref() {
                let path = match q_str(&q, "cursor") {
                    Some(c) => format!("/me/play-history/tracks?limit={limit}&{c}"),
                    None => format!("/me/play-history/tracks?limit={limit}"),
                };
                if let Ok((status, v)) = s.sc_get(&path, Some(t)).await {
                    if (200..300).contains(&status) {
                        let collection: Vec<Value> =
                            sc_items(&v).iter().filter_map(history_entry_from_sc).collect();
                        let next_cursor = v
                            .get("next_href")
                            .and_then(Value::as_str)
                            .and_then(|href| href.split_once('?').map(|(_, query)| query))
                            .map(|query| {
                                query
                                    .split('&')
                                    .filter(|part| !part.starts_with("client_id="))
                                    .collect::<Vec<_>>()
                                    .join("&")
                            })
                            .filter(|query| !query.is_empty());
                        return Ok(ok(
                            json!({ "collection": collection, "next_cursor": next_cursor }),
                        ));
                    }
                }
            }
            let limit = limit as usize;
            let offset = q_u64(&q, "offset", 0) as usize;
            let store = s.store.lock().await;
            let total = store.history.len();
            let slice: Vec<Value> = store
                .history
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect();
            ok(json!({ "collection": slice, "total": total, "next_cursor": null }))
        }
        ("POST", ["history"]) => {
            let mut b = body_json(&body);
            if b.is_object() {
                let now = chrono::Utc::now().to_rfc3339();
                if let Some(obj) = b.as_object_mut() {
                    obj.entry("id").or_insert_with(|| json!(now.clone()));
                    obj.entry("playedAt").or_insert_with(|| json!(now));
                }
                let mut store = s.store.lock().await;
                store.history.insert(0, b.clone());
                store.history.truncate(500);
                store.save();
            }
            // NOTE: the legacy POST /me/play-history write was removed here:
            // SC accepts it (204) but never records it.
            ok(json!({ "ok": true }))
        }
        // Server-side history attribution via the official analytics audio
        // flow (play + checkpoint ~30s apart). The frontend calls this only
        // after ~25s of sustained playback of the same track. Plain HTTPS,
        // no browser involved.
        ("POST", ["history", "report"]) => {
            let b = body_json(&body);
            let page = b.get("permalinkUrl").and_then(Value::as_str).unwrap_or_default();
            let track_urn = b.get("trackUrn").and_then(Value::as_str).unwrap_or_default();
            let owner_urn = b.get("ownerUrn").and_then(Value::as_str).unwrap_or_default();
            let duration_ms = b.get("durationMs").and_then(Value::as_i64).unwrap_or(0);
            let track_id = id_of(track_urn);
            let valid = page.starts_with("https://soundcloud.com/")
                && page.len() < 300
                && track_id.parse::<u64>().is_ok()
                && !owner_urn.is_empty()
                && duration_ms > 0;
            let Some(t) = token.as_deref() else {
                return Ok(ok(json!({ "ok": true, "queued": false })));
            };
            if !valid {
                return Ok(ok(json!({ "ok": true, "queued": false })));
            }
            let my = match need_my_id(s, Some(t)).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let cid = match s.client_id().await {
                Ok(c) => c,
                Err(e) => return Ok(err(502, &e)),
            };
            spawn_history_flow(
                s.http.clone(),
                cid,
                format!("soundcloud:users:{my}"),
                track_id,
                owner_urn.to_string(),
                page.to_string(),
                duration_ms,
            );
            ok(json!({ "ok": true, "queued": true }))
        }
        ("DELETE", ["history"]) => {
            let mut store = s.store.lock().await;
            store.history.clear();
            store.save();
            ok(json!({ "ok": true }))
        }

        // ── misc stubs ──────────────────────────────────────────────
        ("GET", ["events"]) | ("POST", ["events"]) => ok(json!({ "ok": true })),
        ("GET", ["indexing", "stats"]) => ok(json!({ "indexed": 0, "pending": 0 })),
        ("GET", ["dislikes", "ids"]) => {
            let store = s.store.lock().await;
            ok(json!({ "ids": store.disliked }))
        }
        ("GET", ["dislikes", "status", urn]) => {
            let store = s.store.lock().await;
            ok(json!({ "disliked": store.disliked.iter().any(|u| u == urn) }))
        }
        ("POST", ["dislikes", urn]) => {
            let mut store = s.store.lock().await;
            if !store.disliked.iter().any(|u| u == urn) {
                store.disliked.push(urn.to_string());
                store.save();
            }
            ok(json!({ "ok": true }))
        }
        ("DELETE", ["dislikes", urn]) => {
            let mut store = s.store.lock().await;
            store.disliked.retain(|u| u != urn);
            store.save();
            ok(json!({ "ok": true }))
        }

        // ── debug: DataDome write-path probe (never changes account state) ──
        ("POST", ["debug", "write-probe"]) => {
            let Some(t) = token.as_deref() else {
                return Ok(err(401, "unauthorized"));
            };
            let id = q_str(&q, "id").unwrap_or_default();
            let method = q_str(&q, "method").unwrap_or_else(|| "PUT".into());
            let my = match need_my_id(s, Some(t)).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            // Only run a PUT when the track is already liked (idempotent), and
            // only run a DELETE when it is not liked (no-op)  Ethe probe must
            // never change the account state.
            let ids = liked_track_ids(s, t).await;
            let is_liked = ids.iter().any(|x| x == &id);
            if (method == "PUT" && !is_liked) || (method == "DELETE" && is_liked) {
                return Ok(ok(json!({
                    "status": 0,
                    "skipped": "probe would change state",
                    "is_liked": is_liked,
                })));
            }
            let cid = match s.client_id().await {
                Ok(c) => c,
                Err(e) => return Ok(err(502, &e)),
            };
            let url = format!("{SC_API}/users/{my}/track_likes/{id}?client_id={cid}");
            let req = if method == "DELETE" {
                s.http.delete(&url)
            } else {
                s.http.put(&url)
            };
            let mut req = req
                .header("Authorization", format!("OAuth {t}"))
                .header(
                    "User-Agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
                )
                .header("Accept", "application/json, text/javascript, */*; q=0.1")
                .header("Origin", "https://soundcloud.com")
                .header("Referer", "https://soundcloud.com/");
            if let Some(dd) = q_str(&q, "dd").filter(|v| !v.is_empty()) {
                req = req.header("Cookie", format!("datadome={dd}"));
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let datadome = resp
                        .headers()
                        .get("x-datadome")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-")
                        .to_string();
                    let body = resp.text().await.unwrap_or_default();
                    let body: String = body.chars().take(300).collect();
                    ok(json!({
                        "status": status,
                        "x_datadome": datadome,
                        "is_liked": is_liked,
                        "body": body,
                    }))
                }
                Err(e) => err(502, &e.to_string()),
            }
        }

        // ── debug: DataDome webview write-path probe (never changes state) ──
        ("POST", ["debug", "webview-probe"]) => {
            let Some(t) = token.as_deref() else {
                return Ok(err(401, "unauthorized"));
            };
            let id = q_str(&q, "id").unwrap_or_default();
            let method = q_str(&q, "method").unwrap_or_else(|| "PUT".into());
            let my = match need_my_id(s, Some(t)).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let ids = liked_track_ids(s, t).await;
            let is_liked = ids.iter().any(|x| x == &id);
            if (method == "PUT" && !is_liked) || (method == "DELETE" && is_liked) {
                return Ok(ok(json!({
                    "status": 0,
                    "skipped": "probe would change state",
                    "is_liked": is_liked,
                })));
            }
            let cid = match s.client_id().await {
                Ok(c) => c,
                Err(e) => return Ok(err(502, &e)),
            };
            let url = format!("{SC_API}/users/{my}/track_likes/{id}?client_id={cid}");
            let started = Instant::now();
            match super::super::webview::execute(&s.app, Some(t), &method, &url, None).await {
                Ok(o) => {
                    let payload: String = o.payload.chars().take(500).collect();
                    ok(json!({
                        "status": o.status,
                        "captcha": o.captcha,
                        "is_liked": is_liked,
                        "body": payload,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                    }))
                }
                Err(e) => err(502, &e),
            }
        }

        // ── debug: attribute a play by streaming (in-page, with cookies) ──
        ("POST", ["debug", "attr-play"]) => {
            let Some(t) = token.as_deref() else {
                return Ok(err(401, "unauthorized"));
            };
            let id = q_str(&q, "id").unwrap_or_default();
            if id.is_empty() {
                return Ok(err(400, "missing ?id="));
            }
            let cid = match s.client_id().await {
                Ok(c) => c,
                Err(e) => return Ok(err(502, &e)),
            };
            match super::super::webview::attr_play(&s.app, t, &id, &cid).await {
                Ok(v) => ok(v),
                Err(e) => err(502, &e),
            }
        }
        // ── debug: raw fetch through the writer page (cookies+fingerprint) ──
        ("POST", ["debug", "page-fetch"]) => {
            let Some(t) = token.as_deref() else {
                return Ok(err(401, "unauthorized"));
            };
            let url = q_str(&q, "url").unwrap_or_default();
            if url.is_empty() {
                return Ok(err(400, "missing ?url="));
            }
            let method = q_str(&q, "method").unwrap_or_else(|| "POST".into());
            let body: Option<Value> =
                if body.is_empty() { None } else { serde_json::from_slice(body).ok() };
            let ct = q_str(&q, "ct").unwrap_or_default();
            let noauth = q_str(&q, "noauth").unwrap_or_default() == "1";
            let tok = if noauth { None } else { Some(t) };
            let xh: Option<serde_json::Map<String, Value>> = q_str(&q, "xh")
                .filter(|s| !s.is_empty())
                .and_then(|s| serde_json::from_str(&s).ok());
            let out = if ct.is_empty() && xh.is_none() {
                super::super::webview::execute(&s.app, tok, &method, &url, body.as_ref())
                    .await
            } else {
                let ct_opt = if ct.is_empty() { None } else { Some(ct.as_str()) };
                super::super::webview::execute_full(
                    &s.app,
                    tok,
                    &method,
                    &url,
                    body.as_ref(),
                    ct_opt,
                    xh.as_ref(),
                )
                .await
            };
            match out {
                Ok(o) => ok(json!({
                    "status": o.status,
                    "captcha": o.captcha,
                    "body": o.payload.chars().take(500).collect::<String>(),
                })),
                Err(e) => err(502, &e),
            }
        }
        // ── debug: end-to-end hot replay test (capture + immediate re-POST) ──
        ("POST", ["debug", "hot-replay"]) => {
            let Some(t) = token.as_deref() else {
                return Ok(err(401, "unauthorized"));
            };
            let page = q_str(&q, "page").unwrap_or_default();
            if page.is_empty() {
                return Ok(err(400, "missing ?page="));
            }
            match super::super::webview::hot_replay(&s.app, t, &page).await {
                Ok(v) => ok(v),
                Err(e) => err(502, &e),
            }
        }
        // ── debug: sniff the official web player's api-v2 traffic around a play ──
        ("POST", ["debug", "sniff-play"]) => {
            let Some(t) = token.as_deref() else {
                return Ok(err(401, "unauthorized"));
            };
            let page = q_str(&q, "page").unwrap_or_default();
            if page.is_empty() {
                return Ok(err(400, "missing ?page="));
            }
            let wait = q_u64(&q, "wait", 20).clamp(5, 90);
            match super::super::webview::sniff_official_play(&s.app, t, &page, wait).await {
                Ok(v) => ok(v),
                Err(e) => err(502, &e),
            }
        }
        // ── debug: show/hide the writer webview for manual interaction ──
        ("POST", ["debug", "show-writer"]) => {
            if let Some(wv) = super::super::webview::ensure_window(&s.app) {
                if let Some(t) = token.as_deref() {
                    super::super::webview::inject_session(&wv, t);
                }
                let _ = wv.show();
                let _ = wv.set_focus();
                // Reload so the injected session cookie applies to the page.
                if let Ok(url) = "https://soundcloud.com/".parse::<tauri::Url>() {
                    let _ = wv.navigate(url);
                }
                ok(json!({ "ok": true }))
            } else {
                err(500, "writer unavailable")
            }
        }
        ("POST", ["debug", "hide-writer"]) => {
            if let Some(wv) = s.app.get_webview_window(super::super::webview::LABEL) {
                let _ = wv.hide();
            }
            ok(json!({ "ok": true }))
        }

        _ => err(404, "not found"),
    };
    Ok(resp)
}

/// Map one entry of SoundCloud's `/me/play-history/tracks` response into the
/// frontend `HistoryEntry` shape (see `useHistory` / `HistoryTab`).
fn history_entry_from_sc(row: &Value) -> Option<Value> {
    let raw = row.get("track")?;
    if !raw.is_object() {
        return None;
    }
    // normalize first: jacket-less tracks inherit the uploader avatar here too.
    let track = normalize_urn(raw.clone());
    let track_id = track
        .get("id")
        .and_then(Value::as_u64)
        .map(|id| id.to_string())
        .or_else(|| row.get("track_id").and_then(Value::as_u64).map(|id| id.to_string()))?;
    let played_at = row.get("played_at").and_then(Value::as_i64).unwrap_or(0);
    let played_iso = chrono::TimeZone::timestamp_millis_opt(&chrono::Utc, played_at)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();
    let user = track.get("user");
    Some(json!({
        "id": format!("{track_id}:{played_at}"),
        "scTrackId": track_id,
        "title": track.get("title").and_then(Value::as_str).unwrap_or_default(),
        "artistName": user.and_then(|u| u.get("username")).and_then(Value::as_str).unwrap_or_default(),
        "artistUrn": user.and_then(|u| u.get("urn")).and_then(Value::as_str),
        "artworkUrl": track.get("artwork_url"),
        "duration": track.get("duration").and_then(Value::as_i64).unwrap_or(0),
        "playedAt": played_iso,
    }))
}

/// User-Agent for the analytics writes (must look like a real browser).
const HIST_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";

/// NOTE: the JSON is assembled by hand (not `json!`) on purpose: the SC
/// ingestion only honors the official key order, and `serde_json::Map`
/// sorts keys alphabetically (verified: sorted bodies are 200-ignored).
fn audio_event_body(
    track_id: &str,
    owner_urn: &str,
    page_url: &str,
    user_urn: &str,
    duration_ms: i64,
    action: &str,
    playhead_ms: i64,
    queue_id: &str,
    queue_source: &str,
    ts_ms: i64,
    sent_at: &str,
) -> String {
    let s: fn(&str) -> serde_json::Result<String> = serde_json::to_string;
    let urn = format!("soundcloud:tracks:{track_id}");
    format!(
        "{{\"events\":[{{\"event\":\"audio\",\"version\":\"v1.27.47\",\"payload\":{{\"page_name\":\"tracks:main\",\"page_urn\":{},\"source\":\"single\",\"track_length\":{},\"player_type\":\"MaestroHLSMSE\",\"preset\":\"aac_160k\",\"quality\":\"sq\",\"audio_quality_mode\":\"standard\",\"app_state\":\"foreground\",\"action\":{},\"trigger\":\"auto\",\"policy\":\"ALLOW\",\"monetization_model\":\"NOT_APPLICABLE\",\"query_position\":0,\"queue_id\":{},\"queue_source_id\":{},\"track\":{},\"track_owner\":{},\"playhead_position\":{},\"anonymous_id\":\"179330-560771-620681-350488\",\"client_id\":46941,\"ts\":{},\"url\":{},\"session_id\":\"64FB06FC-6301-4316-A4B8-8D523FB0331F\",\"analytics_id\":\"01KJX8TNZEJ7RD9K4KWHTTDB5S\",\"app_version\":\"1791568648\",\"user\":{},\"referrer\":\"https://soundcloud.com/\"}}}}],\"sent_at\":{}}}",
        s(&urn).unwrap_or_default(),
        duration_ms,
        s(action).unwrap_or_default(),
        s(queue_id).unwrap_or_default(),
        s(queue_source).unwrap_or_default(),
        s(&urn).unwrap_or_default(),
        s(owner_urn).unwrap_or_default(),
        playhead_ms,
        ts_ms,
        s(page_url).unwrap_or_default(),
        s(user_urn).unwrap_or_default(),
        s(sent_at).unwrap_or_default(),
    )
}

async fn post_audio_event(http: &wreq::Client, cid: &str, page_url: &str, body: &str) {
    let url = format!("{SC_API}/me?client_id={cid}");
    eprintln!("[history] POST {url}");
    match http
        .post(&url)
        .header("User-Agent", HIST_UA)
        .header("Content-Type", "application/json")
        .header("Origin", "https://soundcloud.com")
        .header("Referer", page_url)
        .body(body.to_string())
        .send()
        .await
    {
        Ok(r) => eprintln!("[history] audio report -> {}", r.status()),
        Err(e) => eprintln!("[history] audio report failed: {e}"),
    }
}

/// Fire-and-forget play attribution: `play` now, `checkpoint` 30s later.
pub(crate) fn spawn_history_flow(
    http: wreq::Client,
    cid: String,
    user_urn: String,
    track_id: String,
    owner_urn: String,
    page_url: String,
    duration_ms: i64,
) {
    tokio::spawn(async move {
        // Proven pair shape: play near position ~1s, checkpoint ~36s later.
        // (Verified 5x: SC records this; other playhead combos are ignored.)
        let queue = uuid::Uuid::new_v4().to_string();
        let qs = uuid::Uuid::new_v4().to_string();
        let sent_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let ts = chrono::Utc::now().timestamp_millis();
        let play = audio_event_body(
            &track_id, &owner_urn, &page_url, &user_urn, duration_ms, "play",
            duration_ms.min(1_000), &queue, &qs, ts, &sent_at,
        );
        post_audio_event(&http, &cid, &page_url, &play).await;
        tokio::time::sleep(Duration::from_secs(35)).await;
        let sent_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let ts = chrono::Utc::now().timestamp_millis();
        let checkpoint = audio_event_body(
            &track_id, &owner_urn, &page_url, &user_urn, duration_ms, "checkpoint",
            duration_ms.min(36_000), &queue, &qs, ts, &sent_at,
        );
        post_audio_event(&http, &cid, &page_url, &checkpoint).await;
    });
}

#[cfg(test)]
mod tests {
    use super::history_entry_from_sc;
    use serde_json::{Value, json};

    fn track(artwork: serde_json::Value) -> serde_json::Value {
        json!({
            "kind": "track",
            "id": 1,
            "title": "t",
            "artwork_url": artwork,
            "user": {
                "username": "u",
                "avatar_url": "https://example.com/avatar-large.jpg",
            },
        })
    }

    /// History entries inherit the fallback too.
    #[test]
    fn history_entry_uses_fallback_artwork() {
        let row = json!({
            "played_at": 1791331200000i64,
            "track": track(serde_json::Value::Null),
        });
        let entry = history_entry_from_sc(&row).expect("entry");
        assert_eq!(
            entry.get("artworkUrl").and_then(|v| v.as_str()),
            Some("https://example.com/avatar-large.jpg"),
        );
    }

    /// SC only honors the official analytics key order (verified: BTreeMap-
    /// sorted bodies are 200-ignored). The hand-built body must keep it.
    #[test]
    fn audio_event_body_keeps_official_key_order() {
        let body = super::audio_event_body(
            "1",
            "soundcloud:users:2",
            "https://soundcloud.com/u/slug",
            "soundcloud:users:3",
            200000,
            "checkpoint",
            30000,
            "q",
            "qs",
            1791568600000,
            "2026-10-10T05:16:40.000Z",
        );
        let order = [
            "\"page_name\"",
            "\"page_urn\"",
            "\"player_type\"",
            "\"action\"",
            "\"queue_id\"",
            "\"track\"",
            "\"track_owner\"",
            "\"playhead_position\"",
            "\"ts\"",
            "\"url\"",
            "\"session_id\"",
            "\"user\"",
            "\"sent_at\"",
        ];
        let mut pos = 0;
        for key in order {
            let at = body[pos..].find(key).expect(key);
            pos += at + key.len();
        }
        assert!(body.starts_with("{\"events\":[{\"event\":\"audio\""));
        // Must be parseable JSON with the exact official shape.
        let v: Value =
            serde_json::from_str(&body).expect("audio body must be valid JSON");
        assert_eq!(v.get("events").and_then(Value::as_array).map(Vec::len), Some(1));
        assert!(v.get("sent_at").and_then(Value::as_str).is_some());
    }
}
