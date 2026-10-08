use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::sc::{SC_API, id_of};
use crate::backend::writer::spawn_write_silent;
use std::time::Instant;

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
            let track_urn = b
                .get("scTrackId")
                .and_then(Value::as_str)
                .map(str::to_string);
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
            if let Some(urn) = track_urn {
                if let Ok(id) = id_of(&urn).parse::<u64>() {
                    if let (Some(t), Ok(cid)) = (token.as_deref(), s.client_id().await) {
                        spawn_write_silent(
                            s.app.clone(),
                            t.to_string(),
                            "POST",
                            format!("{SC_API}/me/play-history?client_id={cid}"),
                            Some(json!({ "track_urn": format!("soundcloud:tracks:{id}") })),
                        );
                    }
                }
            }
            ok(json!({ "ok": true }))
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
            match crate::backend::writer::execute(&s.app, Some(t), &method, &url, None).await {
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

        // Phase 4 (wry writer): challenge-solving UI

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

#[cfg(test)]
mod tests {
    use super::history_entry_from_sc;
    use serde_json::json;

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
}
