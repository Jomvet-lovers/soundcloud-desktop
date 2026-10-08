use std::sync::Arc;

use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::DirectState;
use super::super::sc::{SC_API, id_of, fetch_me};
use crate::backend::writer::emit_sync_error;
use std::time::{Duration, Instant};

pub async fn route(ctx: &Ctx) -> Result<Response, warp::Rejection> {
    let s = &ctx.state;
    let state = &ctx.state;
    let q = &ctx.q;
    let token = &ctx.token;
    let body = &ctx.body;
    let p: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (ctx.method.as_str(), p.as_slice()) {
        ("GET", ["tracks"]) if q.contains_key("ids") => {
            let ids: Vec<String> = q
                .get("ids")
                .map(|raw| raw.split(',').map(id_of).collect())
                .unwrap_or_default();
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(&format!("/tracks?ids={}", ids.join(",")), Some(t))
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => {
                        let items: Vec<Value> = v
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(normalize_urn)
                            .collect();
                        ok(page(items, page_no, limit, false))
                    }
                    Ok((status, v)) => json_resp(status, &v),
                    Err(e) => err(502, &e),
                },
                None => match s.sc_get(&format!("/tracks?ids={}", ids.join(",")), None).await {
                    Ok((_status, v)) => {
                        let items: Vec<Value> = v
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(normalize_urn)
                            .collect();
                        ok(page(items, page_no, limit, false))
                    }
                    Err(e) => err(502, &e),
                },
            }
        }
        ("GET", ["tracks", urn]) => {
            let id = id_of(urn);
            match s.sc_get(&format!("/tracks/{id}"), token.as_deref()).await {
                Ok((status, v)) => json_resp(status, &normalize_urn(v)),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["tracks", urn, "related"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 10);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/tracks/{id}/related?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                _ => ok(empty_page(page_no, limit)),
            }
        }
        ("GET", ["tracks", urn, "favoriters"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 12);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/tracks/{id}/favoriters?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                _ => ok(empty_page(page_no, limit)),
            }
        }
        ("GET", ["tracks", urn, "comments"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 20);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            // `threaded=1` is required by api-v2 (without it SoundCloud 400s).
            let (sc_comments, sc_more) = match s
                .sc_get(
                    &format!("/tracks/{id}/comments?limit={limit}&offset={offset}&threaded=1"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => (
                    sc_items(&v)
                        .into_iter()
                        .map(normalize_comment)
                        .collect::<Vec<_>>(),
                    sc_has_more(&v),
                ),
                _ => (Vec::new(), false),
            };

            let mut items: Vec<Value> = Vec::new();
            if page_no == 0 {
                let store = s.store.lock().await;
                for c in store
                    .comments
                    .iter()
                    .filter(|c| c.get("track_urn").and_then(Value::as_str) == Some(urn))
                {
                    if local_comment_synced_elsewhere(c, &sc_comments) {
                        continue;
                    }
                    let mut c = c.clone();
                    if c.get("user").map(Value::is_null).unwrap_or(true) {
                        c["user"] = local_comment_user_placeholder();
                    }
                    items.push(c);
                }
            }
            items.extend(sc_comments);
            ok(page(items, page_no, limit, sc_more))
        }
        ("POST", ["tracks", urn, "comments"]) => {
            let b = body_json(&body);
            let comment = b.get("comment").cloned().unwrap_or(b);
            let text = comment
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if text.is_empty() {
                err(400, "empty comment")
            } else {
                let timestamp = comment
                    .get("timestamp")
                    .and_then(Value::as_u64)
                    .filter(|t| *t > 0);
                let now = chrono::Utc::now();
                let temp_id = now.timestamp_millis();
                let user = match token.as_deref() {
                    Some(t) => fetch_me(s, t).await.ok().map(|me| {
                        json!({
                            "id": me.get("id").cloned().unwrap_or(Value::Null),
                            "urn": me.get("urn").cloned().unwrap_or(Value::Null),
                            "username": me.get("username").cloned().unwrap_or(Value::Null),
                            "avatar_url": me.get("avatar_url").cloned().unwrap_or(Value::Null),
                            "permalink_url": me.get("permalink_url").cloned().unwrap_or(Value::Null),
                        })
                    }),
                    None => None,
                };
                let entry = json!({
                    "id": temp_id,
                    "urn": format!("local:comments:{temp_id}"),
                    "track_urn": urn,
                    "body": text.clone(),
                    "timestamp": timestamp,
                    "created_at": now.to_rfc3339(),
                    "user": user,
                    "sync": "pending",
                });
                {
                    let mut store = s.store.lock().await;
                    store.comments.insert(0, entry.clone());
                    store.save();
                }

                // Best-effort SoundCloud write through the hidden writer webview.
                // The local copy stays visible until (or unless) it lands there.
                if let Some(t) = token.clone() {
                    if let Ok(cid) = s.client_id().await {
                        tokio::spawn(sync_local_comment(
                            state.clone(),
                            t,
                            id_of(urn),
                            cid,
                            urn.to_string(),
                            temp_id,
                            text.clone(),
                            timestamp.unwrap_or(0),
                        ));
                    }
                }
                ok(entry)
            }
        }
        ("POST", ["tracks", urn, "comments", "sync"]) => {
            // Re-attempt the SoundCloud write for this track's local comments
            // (pending after a failure or an offline session).
            let mut queued: Vec<(i64, Value)> = Vec::new();
            {
                let mut store = s.store.lock().await;
                for c in store.comments.iter_mut().filter(|c| {
                    c.get("track_urn").and_then(Value::as_str) == Some(urn)
                        && matches!(
                            c.get("sync").and_then(Value::as_str),
                            Some("pending") | Some("failed")
                        )
                }) {
                    if let Some(id) = c.get("id").and_then(Value::as_i64) {
                        c["sync"] = json!("pending");
                        queued.push((id, c.clone()));
                    }
                }
                if !queued.is_empty() {
                    store.save();
                }
            }
            if let Some(t) = token.as_deref() {
                if let Ok(cid) = s.client_id().await {
                    let track_id = id_of(urn);
                    for (temp_id, c) in &queued {
                        tokio::spawn(sync_local_comment(
                            state.clone(),
                            t.to_string(),
                            track_id.clone(),
                            cid.clone(),
                            urn.to_string(),
                            *temp_id,
                            c.get("body").and_then(Value::as_str).unwrap_or_default().to_string(),
                            c.get("timestamp").and_then(Value::as_u64).unwrap_or(0),
                        ));
                    }
                }
            }
            ok(json!({ "retried": queued.len() }))
        }
        ("DELETE", ["comments", comment_id]) => {
            // Delete a comment: best-effort SoundCloud delete (the web client
            // uses DELETE /comments/:id), then drop the local copy.
            let id: i64 = comment_id.parse().unwrap_or(0);
            if id == 0 {
                err(400, "bad comment id")
            } else {
                let (local, local_synced) = {
                    let store = s.store.lock().await;
                    match store.comments.iter().find(|c| {
                        c.get("id").and_then(Value::as_i64) == Some(id)
                            || c.get("sc_id").and_then(Value::as_i64) == Some(id)
                    }) {
                        Some(c) => (
                            true,
                            c.get("sync").and_then(Value::as_str) == Some("synced"),
                        ),
                        None => (false, false),
                    }
                };
                // Never synced (temp local id) → nothing to do remotely.
                if !local || local_synced {
                    match (token.as_deref(), s.client_id().await) {
                        (Some(t), Ok(cid)) => {
                            let url = format!(
                                "{SC_API}/comments/{id}?client_id={cid}&app_version={}",
                                super::super::sc::app_version(&s.http).await
                            );
                            match crate::backend::writer::execute(&s.app, Some(t), "DELETE", &url, None).await
                            {
                                Ok(r) if (200..300).contains(&r.status) || r.status == 404 => {}
                                Ok(r) => {
                                    emit_sync_error(
                                        &s.app,
                                        "DELETE",
                                        &url,
                                        r.status,
                                        r.captcha.is_some(),
                                        None,
                                    );
                                    return Ok(err(502, "SoundCloud delete failed"));
                                }
                                Err(e) => {
                                    emit_sync_error(&s.app, "DELETE", &url, 0, false, Some(&e));
                                    return Ok(err(502, &e));
                                }
                            }
                        }
                        _ => return Ok(err(401, "unauthorized")),
                    }
                }
                {
                    let mut store = s.store.lock().await;
                    store.comments.retain(|c| {
                        c.get("id").and_then(Value::as_i64) != Some(id)
                            && c.get("sc_id").and_then(Value::as_i64) != Some(id)
                    });
                    store.save();
                }
                ok(json!({ "ok": true }))
            }
        }
        ("GET", ["tags", tag, "tracks"]) => {
            // Tag feed (the web tag pages' "recent tracks"), widened and sorted
            // locally so the tag page can offer the same paging/sorting as the
            // search page. `limit` maxes out at 50 per request, so the window
            // is built by following the feed cursor.
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = (page_no * limit) as usize;
            let sort = q_str(&q, "sort").unwrap_or_else(|| "newest".to_string());

            let cache_key = format!("tag\u{1}{tag}\u{1}{sort}");
            let cached = {
                let cache = s.search_cache.lock().await;
                cache.get(&cache_key).cloned()
            };
            let all = match cached {
                Some((at, items)) if at.elapsed() < Duration::from_secs(300) => items,
                _ => {
                    let mut items: Vec<Value> = Vec::new();
                    let mut cursor: Option<String> = None;
                    for _ in 0..4 {
                        let mut path =
                            format!("/recent-tracks/{}?limit=50", urlencoding::encode(tag));
                        if let Some(c) = cursor.as_deref() {
                            path.push_str("&offset=");
                            path.push_str(&urlencoding::encode(c));
                        }
                        let (status, v) = match s.sc_get(&path, token.as_deref()).await {
                            Ok(x) => x,
                            Err(_) => break,
                        };
                        if !(200..300).contains(&status) {
                            break;
                        }
                        items.extend(
                            sc_items(&v)
                                .into_iter()
                                .filter(|it| {
                                    it.get("kind").and_then(Value::as_str) != Some("playlist")
                                })
                                .map(normalize_urn)
                                .map(normalize_deep),
                        );
                        cursor = v.get("next_href").and_then(Value::as_str).and_then(|u| {
                            url::Url::parse(u).ok().and_then(|parsed| {
                                parsed
                                    .query_pairs()
                                    .find(|(k, _)| k == "offset")
                                    .map(|(_, val)| val.into_owned())
                            })
                        });
                        if cursor.is_none() || items.len() >= 200 {
                            break;
                        }
                    }
                    let mut seen = std::collections::HashSet::new();
                    items.retain(|it| {
                        let urn = it
                            .get("urn")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        seen.insert(urn)
                    });
                    sort_tracks(&mut items, &sort);
                    let mut cache = s.search_cache.lock().await;
                    if cache.len() > 24 {
                        cache.clear();
                    }
                    cache.insert(cache_key, (Instant::now(), items.clone()));
                    items
                }
            };
            let total = all.len() as u64;
            let slice: Vec<Value> = all.into_iter().skip(offset).take(limit as usize).collect();
            let has_more = (offset as u64 + slice.len() as u64) < total;
            ok(page(slice, page_no, limit, has_more))
        }
        ("GET", ["tracks", _urn, "sharing"]) => ok(json!({ "sharing": "public" })),
        ("PUT", ["tracks", _urn, "sharing"]) | ("DELETE", ["tracks", _urn, "sharing"]) => {
            ok(json!({ "ok": true }))
        }

        // ── playlists ───────────────────────────────────────────────

        _ => err(404, "not found"),
    };
    Ok(resp)
}

async fn sync_local_comment(
    state: Arc<DirectState>,
    token: String,
    track_id: String,
    client_id: String,
    track_urn: String,
    temp_id: i64,
    body_text: String,
    timestamp_ms: u64,
) {
    // The web client sends comment writes as QUERY parameters (`body` and
    // `timestamp`), not a JSON body — a JSON body answers 400 "missing
    // params". `app_version` is required by the same validation.
    let url = format!(
        "{SC_API}/tracks/{track_id}/comments?client_id={client_id}&app_version={}&body={}&timestamp={timestamp_ms}",
        super::super::sc::app_version(&state.http).await,
        urlencoding::encode(&body_text)
    );
    let (synced, sc_id) = match crate::backend::writer::execute(&state.app, Some(&token), "POST", &url, None).await {
        Ok(r) if (200..300).contains(&r.status) => {
            let sc_id = parse_sc_comment_id(&r.payload);
            eprintln!("[comments] synced {temp_id} -> {sc_id:?}");
            (true, sc_id)
        }
        Ok(r) => {
            eprintln!(
                "[comments] sync failed {temp_id}: status={} captcha={} body={}",
                r.status,
                r.captcha.is_some(),
                r.payload.chars().take(800).collect::<String>()
            );
            emit_sync_error(&state.app, "POST", &url, r.status, r.captcha.is_some(), None);
            (false, None)
        }
        Err(e) => {
            eprintln!("[comments] sync error {temp_id}: {e}");
            emit_sync_error(&state.app, "POST", &url, 0, false, Some(&e));
            (false, None)
        }
    };

    {
        let mut store = state.store.lock().await;
        if let Some(c) = store.comments.iter_mut().find(|c| {
            c.get("id").and_then(Value::as_i64) == Some(temp_id)
                && c.get("sync").and_then(Value::as_str) == Some("pending")
        }) {
            if synced {
                if let Some(id) = sc_id {
                    c["id"] = json!(id);
                    c["urn"] = json!(format!("soundcloud:comments:{id}"));
                    c["sc_id"] = json!(id);
                }
                c["sync"] = json!("synced");
            } else {
                c["sync"] = json!("failed");
            }
            store.save();
        }
    }

    if synced {
        state.app.emit(
            "direct:comments-synced",
            json!({ "track_urn": track_urn }),
        );
    }
}
