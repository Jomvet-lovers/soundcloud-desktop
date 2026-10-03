//! Local HTTP API implementing the subset of the app backend that the
//! frontend uses. Reads are resolved against SoundCloud api-v2; writes are
//! persisted locally in [`super::store::LocalStore`].

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use serde_json::{json, Value};
use warp::http::{HeaderMap, Method, StatusCode};
use warp::reply::{Reply, Response};
use warp::{path::FullPath, Filter};

use super::sc::{fetch_me, id_of};
use super::DirectState;

pub async fn start(state: Arc<DirectState>) -> u16 {
    let routes = warp::any()
        .and(warp::method())
        .and(warp::path::full())
        .and(warp::query::raw())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and(warp::any().map(move || state.clone()))
        .and_then(handle)
        .with(crate::network::server::cors());

    let (addr, server) = warp::serve(routes).bind_ephemeral(([127, 0, 0, 1], 0));
    tokio::spawn(server);
    println!("[DirectAPI] http://127.0.0.1:{}", addr.port());
    addr.port()
}

// ─── helpers ─────────────────────────────────────────────────────────────

fn json_resp(code: u16, v: &Value) -> Response {
    let mut resp = warp::reply::json(v).into_response();
    *resp.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::OK);
    resp
}

fn ok(v: Value) -> Response {
    json_resp(200, &v)
}

fn err(code: u16, msg: &str) -> Response {
    json_resp(code, &json!({ "error": msg }))
}

fn page(items: Vec<Value>, page: u64, limit: u64, has_more: bool) -> Value {
    json!({
        "collection": items,
        "page": page,
        "page_size": limit,
        "has_more": has_more,
    })
}

fn empty_page(page_no: u64, limit: u64) -> Value {
    page(Vec::new(), page_no, limit, false)
}

fn q_u64(q: &HashMap<String, String>, key: &str, default: u64) -> u64 {
    q.get(key)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn q_str(q: &HashMap<String, String>, key: &str) -> Option<String> {
    q.get(key).filter(|v| !v.is_empty()).cloned()
}

fn parse_query(raw: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(raw.as_bytes())
        .into_owned()
        .collect()
}

fn token_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|t| !t.is_empty() && t != "undefined" && t != "null")
}

fn body_json(body: &Bytes) -> Value {
    if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(body).unwrap_or(Value::Null)
    }
}

fn sc_items(v: &Value) -> Vec<Value> {
    v.get("collection")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn sc_has_more(v: &Value) -> bool {
    v.get("next_href")
        .map(|n| n.is_string() && !n.is_null())
        .unwrap_or(false)
}

fn sc_user_items(v: &Value) -> Vec<Value> {
    sc_items(v)
}

fn normalize_urn(mut item: Value) -> Value {
    if item.get("urn").is_none() {
        if let Some(id) = item.get("id").and_then(Value::as_u64) {
            let kind = match item.get("kind").and_then(Value::as_str) {
                Some("user") | Some("artist") => "users",
                Some("playlist") | Some("album") => "playlists",
                _ => "tracks",
            };
            if let Some(obj) = item.as_object_mut() {
                obj.insert("urn".into(), json!(format!("soundcloud:{kind}:{id}")));
            }
        }
    }
    item
}

fn urn_of(v: &Value) -> Option<String> {
    v.get("urn").and_then(Value::as_str).map(str::to_string)
}

async fn need_my_id(state: &DirectState, token: Option<&str>) -> Result<u64, Response> {
    let Some(t) = token else {
        return Err(err(401, "unauthorized"));
    };
    state
        .my_user_id(t)
        .await
        .map_err(|e| json_resp(502, &json!({ "error": e })))
}

// ─── dispatch ────────────────────────────────────────────────────────────

async fn handle(
    method: Method,
    full: FullPath,
    raw_query: String,
    headers: HeaderMap,
    body: Bytes,
    state: Arc<DirectState>,
) -> Result<Response, warp::Rejection> {
    let raw_path = full.as_str().split('?').next().unwrap_or("").to_string();
    let decoded: Vec<String> = raw_path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| urlencoding::decode(s).map(|c| c.into_owned()).unwrap_or_else(|_| s.to_string()))
        .collect();
    let segs: Vec<&str> = decoded.iter().map(String::as_str).collect();
    let q = parse_query(&raw_query);
    let token = token_of(&headers);
    let m = method.as_str();
    let s = state.as_ref();

    let resp = match (m, segs.as_slice()) {
        // ── meta ────────────────────────────────────────────────────
        ("GET", ["health"]) => ok(json!({"status": "ok"})),

        // ── auth ────────────────────────────────────────────────────
        ("GET", ["auth", "status"]) => ok(json!({
            "authenticated": token.is_some(),
            "tokenState": "ok",
            "pendingSyncCount": 0,
            "failedSyncCount": 0,
        })),
        ("POST", ["auth", "refresh"]) => ok(json!({})),
        ("POST", ["auth", "logout"]) => ok(json!({})),
        ("POST", ["auth", "link", "create"]) | ("POST", ["auth", "link", "claim"]) => {
            err(501, "direct mode: not supported")
        }

        // ── me ──────────────────────────────────────────────────────
        ("GET", ["me", "cold"]) => match token.as_deref() {
            None => err(401, "unauthorized"),
            Some(t) => match fetch_me(s, t).await {
                Ok(mut me) => {
                    let store = s.store.lock().await;
                    if let Some(obj) = me.as_object_mut() {
                        let liked = store.liked_tracks.len() as i64;
                        let unliked = store.unliked_tracks.len() as i64;
                        if let Some(Value::Number(n)) = obj.get_mut("likes_count") {
                            let base = n.as_i64().unwrap_or(0);
                            *n = serde_json::Number::from((base + liked - unliked).max(0));
                        } else if let Some(Value::Number(n)) = obj.get_mut("public_favorites_count") {
                            let base = n.as_i64().unwrap_or(0);
                            *n = serde_json::Number::from((base + liked - unliked).max(0));
                        }
                    }
                    ok(normalize_urn(me))
                }
                Err(e) => json_resp(502, &json!({ "error": e })),
            },
        },
        ("GET", ["me", "subscription"]) => ok(json!({ "premium": false })),

        ("GET", ["me", "aura"]) => ok(json!({ "aura_id": null, "custom_hex": null })),
        ("PUT", ["me", "aura"]) => {
            let b = body_json(&body);
            ok(json!({
                "aura_id": b.get("aura_id").cloned().unwrap_or(Value::Null),
                "custom_hex": b.get("custom_hex").cloned().unwrap_or(Value::Null),
            }))
        }

        ("GET", ["me", "followings"]) => {
            let my = match need_my_id(s, token.as_deref()).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let limit = q_u64(&q, "limit", 50);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(
                        &format!("/users/{my}/followings?limit={limit}&offset={offset}"),
                        Some(t),
                    )
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => {
                        let store = s.store.lock().await;
                        let items: Vec<Value> = sc_user_items(&v)
                            .into_iter()
                            .filter(|u| {
                                urn_of(u)
                                    .map(|urn| !store.unfollowed.iter().any(|x| x == &urn))
                                    .unwrap_or(true)
                            })
                            .collect();
                        ok(page(items, page_no, limit, sc_has_more(&v)))
                    }
                    Ok((status, v)) => json_resp(status, &v),
                    Err(e) => err(502, &e),
                },
                None => err(401, "unauthorized"),
            }
        }

        ("GET", ["me", "followings", "tracks"]) => {
            let limit = q_u64(&q, "limit", 20);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(&format!("/stream?limit={limit}&offset={offset}"), Some(t))
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => {
                        let tracks = stream_to_tracks(&v);
                        ok(page(tracks, page_no, limit, sc_has_more(&v)))
                    }
                    Ok((status, v)) => json_resp(status, &v),
                    Err(e) => err(502, &e),
                },
                None => err(401, "unauthorized"),
            }
        }

        ("PUT", ["me", "followings", urn]) => {
            let mut store = s.store.lock().await;
            store.unfollowed.retain(|u| u != urn);
            if !store.followed.iter().any(|u| u == urn) {
                store.followed.push(urn.to_string());
                store.save();
            }
            ok(json!({ "ok": true }))
        }
        ("DELETE", ["me", "followings", urn]) => {
            let mut store = s.store.lock().await;
            store.followed.retain(|u| u != urn);
            if !store.unfollowed.iter().any(|u| u == urn) {
                store.unfollowed.push(urn.to_string());
                store.save();
            }
            ok(json!({ "ok": true }))
        }

        ("GET", ["me", "likes", "tracks"]) => {
            let my = match need_my_id(s, token.as_deref()).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(
                        &format!("/users/{my}/track_likes?limit={limit}&offset={offset}"),
                        Some(t),
                    )
                    .await
                {
                    Ok((_status, v)) => {
                        let items: Vec<Value> = sc_items(&v)
                            .into_iter()
                            .filter_map(|it| it.get("track").cloned())
                            .filter(|t| !t.is_null())
                            .map(normalize_urn)
                            .collect();
                        let items = merge_local_likes(s, items, page_no).await;
                        ok(page(items, page_no, limit, sc_has_more(&v)))
                    }
                    Err(e) => err(502, &e),
                },
                None => err(401, "unauthorized"),
            }
        }

        ("GET", ["me", "likes", "playlists"]) => {
            let my = match need_my_id(s, token.as_deref()).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(
                        &format!("/users/{my}/likes?limit={limit}&offset={offset}"),
                        Some(t),
                    )
                    .await
                {
                    Ok((_status, v)) => {
                        let items: Vec<Value> = sc_items(&v)
                            .into_iter()
                            .filter_map(|it| it.get("playlist").cloned())
                            .filter(|p| !p.is_null())
                            .map(normalize_urn)
                            .collect();
                        let items = merge_local_playlist_likes(s, items, page_no).await;
                        ok(page(items, page_no, limit, sc_has_more(&v)))
                    }
                    Err(e) => err(502, &e),
                },
                None => err(401, "unauthorized"),
            }
        }

        ("GET", ["me", "playlists"]) => {
            let my = match need_my_id(s, token.as_deref()).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let limit = q_u64(&q, "limit", 50);
            let page_no = q_u64(&q, "page", 0);
            let mut items: Vec<Value> = Vec::new();
            if page_no == 0 {
                let store = s.store.lock().await;
                items.extend(store.playlists.iter().cloned());
            }
            let sc = match token.as_deref() {
                Some(t) => s
                    .sc_get_opt(&format!("/users/{my}/playlists?limit={limit}&offset=0"), Some(t))
                    .await,
                None => None,
            };
            if let Some(v) = sc {
                let store = s.store.lock().await;
                let sc_items: Vec<Value> = sc_items(&v)
                    .into_iter()
                    .map(normalize_urn)
                    .filter(|p| {
                        urn_of(p)
                            .map(|u| {
                                !store.deleted_playlists.iter().any(|x| x == &u)
                                    && !store.playlists.iter().any(|lp| urn_of(lp).as_deref() == Some(u.as_str()))
                            })
                            .unwrap_or(true)
                    })
                    .collect();
                items.extend(sc_items);
            }
            ok(page(items, page_no, limit, false))
        }

        ("GET", ["me", "tracks"]) => {
            let my = match need_my_id(s, token.as_deref()).await {
                Ok(v) => v,
                Err(r) => return Ok(r),
            };
            let limit = q_u64(&q, "limit", 50);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(&format!("/users/{my}/tracks?limit={limit}&offset={offset}"), Some(t))
                    .await
                {
                    Ok((_status, v)) => {
                        let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                        ok(page(items, page_no, limit, sc_has_more(&v)))
                    }
                    Err(e) => err(502, &e),
                },
                None => err(401, "unauthorized"),
            }
        }

        // ── likes on tracks / playlists ─────────────────────────────
        ("GET", ["likes", "tracks", urn]) => {
            let store = s.store.lock().await;
            ok(json!({ "liked": store.is_liked(urn) }))
        }
        ("PUT", ["likes", "tracks", urn]) => {
            let id = id_of(urn);
            let track = match token.as_deref() {
                Some(t) => s
                    .sc_get_opt(&format!("/tracks/{id}"), Some(t))
                    .await
                    .unwrap_or_else(|| json!({ "urn": urn, "id": id.parse::<u64>().unwrap_or(0) })),
                None => json!({ "urn": urn, "id": id.parse::<u64>().unwrap_or(0) }),
            };
            s.store.lock().await.like_track(normalize_urn(track));
            ok(json!({ "ok": true }))
        }
        ("DELETE", ["likes", "tracks", urn]) => {
            s.store.lock().await.unlike_track(urn);
            ok(json!({ "ok": true }))
        }

        ("GET", ["likes", "playlists", urn]) => {
            let store = s.store.lock().await;
            ok(json!({ "liked": store.is_playlist_liked(urn) }))
        }
        ("PUT", ["likes", "playlists", urn]) => {
            let id = id_of(urn);
            let playlist = match token.as_deref() {
                Some(t) => s
                    .sc_get_opt(&format!("/playlists/{id}"), Some(t))
                    .await
                    .unwrap_or_else(|| json!({ "urn": urn })),
                None => json!({ "urn": urn }),
            };
            s.store.lock().await.like_playlist(normalize_urn(playlist));
            ok(json!({ "ok": true }))
        }
        ("DELETE", ["likes", "playlists", urn]) => {
            s.store.lock().await.unlike_playlist(urn);
            ok(json!({ "ok": true }))
        }

        // ── tracks ──────────────────────────────────────────────────
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
            let mut items: Vec<Value> = Vec::new();
            if page_no == 0 {
                let store = s.store.lock().await;
                items.extend(
                    store
                        .comments
                        .iter()
                        .filter(|c| {
                            c.get("track_urn").and_then(Value::as_str) == Some(urn)
                        })
                        .cloned(),
                );
            }
            match s
                .sc_get(
                    &format!("/tracks/{id}/comments?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    items.extend(sc_items(&v));
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                _ => ok(page(items, page_no, limit, false)),
            }
        }
        ("POST", ["tracks", urn, "comments"]) => {
            let b = body_json(&body);
            let comment = b.get("comment").cloned().unwrap_or(b);
            let now = chrono::Utc::now();
            let entry = json!({
                "id": now.timestamp_millis(),
                "urn": format!("local:comments:{}", now.timestamp_millis()),
                "track_urn": urn,
                "body": comment.get("body").cloned().unwrap_or(Value::Null),
                "timestamp": comment.get("timestamp").cloned().unwrap_or(Value::Null),
                "created_at": now.to_rfc3339(),
                "user": Value::Null,
            });
            let mut store = s.store.lock().await;
            store.comments.insert(0, entry.clone());
            store.save();
            ok(entry)
        }
        ("GET", ["tracks", urn, "sharing"]) => ok(json!({ "sharing": "public" })),
        ("PUT", ["tracks", urn, "sharing"]) | ("DELETE", ["tracks", urn, "sharing"]) => {
            ok(json!({ "ok": true }))
        }

        // ── playlists ───────────────────────────────────────────────
        ("GET", ["playlists"]) => {
            let store = s.store.lock().await;
            let items = store.playlists.clone();
            ok(page(items, 0, 50, false))
        }
        ("POST", ["playlists"]) => {
            let b = body_json(&body);
            let input = b.get("playlist").cloned().unwrap_or(b);
            let me = match token.as_deref() {
                Some(t) => fetch_me(s, t).await.ok(),
                None => None,
            };
            let mut store = s.store.lock().await;
            let urn = store.next_local_playlist_urn();
            let user = me
                .as_ref()
                .map(|u| {
                    json!({
                        "id": u.get("id").cloned().unwrap_or(Value::Null),
                        "urn": u.get("urn").cloned().unwrap_or(Value::Null),
                        "username": u.get("username").cloned().unwrap_or(Value::Null),
                        "avatar_url": u.get("avatar_url").cloned().unwrap_or(Value::Null),
                        "permalink_url": u.get("permalink_url").cloned().unwrap_or(Value::Null),
                    })
                })
                .unwrap_or(Value::Null);
            let playlist = json!({
                "id": urn.rsplit(':').next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0),
                "urn": urn,
                "title": input.get("title").cloned().unwrap_or(json!("Untitled playlist")),
                "description": input.get("description").cloned().unwrap_or(Value::Null),
                "genre": input.get("genre").cloned().unwrap_or(json!("")),
                "tag_list": input.get("tag_list").cloned().unwrap_or(json!("")),
                "sharing": input.get("sharing").cloned().unwrap_or(json!("private")),
                "duration": 0,
                "artwork_url": Value::Null,
                "track_count": 0,
                "created_at": chrono::Utc::now().to_rfc3339(),
                "last_modified": chrono::Utc::now().to_rfc3339(),
                "playlist_type": "playlist",
                "user_favorite": false,
                "tracks": [],
                "user": user,
                "local": true,
            });
            store.upsert_playlist(playlist.clone());
            ok(playlist)
        }
        ("GET", ["playlists", urn]) => {
            let store = s.store.lock().await;
            if store.is_deleted_playlist(urn) {
                return Ok(err(404, "playlist deleted"));
            }
            if let Some(local) = store.find_playlist(urn) {
                return Ok(ok(local));
            }
            drop(store);
            let id = id_of(urn);
            match s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await {
                Ok((status, v)) => json_resp(status, &normalize_urn(v)),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["playlists", urn, "tracks"]) => {
            let limit = q_u64(&q, "limit", 200);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            let local = {
                let store = s.store.lock().await;
                store.find_playlist(urn)
            };
            if let Some(local) = local {
                let all: Vec<Value> = local
                    .get("tracks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let total = all.len() as u64;
                let slice: Vec<Value> = all
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect();
                let has_more = offset + (slice.len() as u64) < total;
                return Ok(ok(page(slice, page_no, limit, has_more)));
            }
            let id = id_of(urn);
            match s
                .sc_get(
                    &format!("/playlists/{id}/tracks?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((_status, v)) => {
                    let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                Err(e) => err(502, &e),
            }
        }
        ("PUT", ["playlists", urn]) | ("PUT", ["playlists", urn, "tracks"]) => {
            let b = body_json(&body);
            let input = b.get("playlist").cloned().unwrap_or(b);
            let mut store = s.store.lock().await;
            let mut playlist = store
                .find_playlist(urn)
                .unwrap_or_else(|| json!({ "urn": urn, "tracks": [], "local": true }));
            if let (Some(dst), Some(src)) = (playlist.as_object_mut(), input.as_object()) {
                for (k, v) in src {
                    if k == "tracks" {
                        continue;
                    }
                    dst.insert(k.clone(), v.clone());
                }
            }
            // `tracks: [{id}]` -> resolve ids to local track objects when we
            // have them cached in likes; otherwise keep the refs.
            if let Some(refs) = input.get("tracks").and_then(Value::as_array) {
                let store_likes = store.liked_tracks.clone();
                let tracks: Vec<Value> = refs
                    .iter()
                    .filter_map(|r| {
                        let id = r
                            .get("id")
                            .or_else(|| r.get("track_id"))
                            .map(|v| v.to_string().trim_matches('"').to_string());
                        id.and_then(|id| {
                            store_likes.iter().find(|t| {
                                t.get("id").map(|v| v.to_string().trim_matches('"') == id.as_str()).unwrap_or(false)
                            }).cloned()
                        })
                    })
                    .collect();
                if !tracks.is_empty() {
                    if let Some(obj) = playlist.as_object_mut() {
                        obj.insert("tracks".into(), json!(tracks));
                        obj.insert("track_count".into(), json!(tracks.len()));
                    }
                }
            }
            store.upsert_playlist(playlist.clone());
            ok(playlist)
        }
        ("DELETE", ["playlists", urn]) => {
            s.store.lock().await.delete_playlist(urn);
            ok(json!({ "ok": true }))
        }
        ("PUT", ["playlists", urn, "sharing"]) => {
            let b = body_json(&body);
            let sharing = b
                .get("sharing")
                .cloned()
                .unwrap_or(json!("private"));
            let mut store = s.store.lock().await;
            if let Some(mut playlist) = store.find_playlist(urn) {
                if let Some(obj) = playlist.as_object_mut() {
                    obj.insert("sharing".into(), sharing);
                }
                store.upsert_playlist(playlist);
            }
            ok(json!({ "ok": true }))
        }

        // ── users ───────────────────────────────────────────────────
        ("GET", ["users", urn]) => {
            let id = id_of(urn);
            match s.sc_get(&format!("/users/{id}"), token.as_deref()).await {
                Ok((status, v)) => json_resp(status, &normalize_urn(v)),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["users", urn, "web-profiles"]) => {
            let id = id_of(urn);
            match s
                .sc_get(&format!("/users/{id}/web-profiles"), token.as_deref())
                .await
            {
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["users", urn, "subscription"]) => ok(json!({ "premium": false })),
        ("GET", ["users", _urn, "aura"]) => ok(json!({ "aura_id": null, "custom_hex": null })),
        ("GET", ["users", urn, "followings"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 50);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/users/{id}/followings?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["users", urn, "tracks"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/users/{id}/tracks?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["users", urn, "playlists"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/users/{id}/playlists?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["users", urn, "track_likes"]) | ("GET", ["users", urn, "likes"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/users/{id}/track_likes?limit={limit}&offset={offset}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items: Vec<Value> = sc_items(&v)
                        .into_iter()
                        .filter_map(|it| it.get("track").cloned())
                        .filter(|t| !t.is_null())
                        .map(normalize_urn)
                        .collect();
                    let items = merge_local_likes(s, items, page_no).await;
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                _ => ok(empty_page(page_no, limit)),
            }
        }
        ("GET", ["users", urn, "followers"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 50);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match s
                .sc_get(
                    &format!("/users/{id}/followers?limit={limit}&offset={offset}"),
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

        // ── albums / artists (catalog stubs) ────────────────────────
        ("GET", ["albums", urn]) => {
            let id = id_of(urn);
            match s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await {
                Ok((status, v)) => json_resp(status, &normalize_urn(v)),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["artists", urn, "albums"]) => {
            let id = id_of(urn);
            match s
                .sc_get(
                    &format!("/users/{id}/playlists?limit=50&offset=0"),
                    token.as_deref(),
                )
                .await
            {
                Ok((_status, v)) => {
                    let items: Vec<Value> = sc_items(&v)
                        .into_iter()
                        .filter(|p| {
                            p.get("is_album").and_then(Value::as_bool).unwrap_or(false)
                                || matches!(
                                    p.get("kind").and_then(Value::as_str),
                                    Some("album") | Some("ep") | Some("single") | Some("compilation")
                                )
                        })
                        .map(normalize_urn)
                        .collect();
                    ok(json!(items))
                }
                Err(_) => ok(json!([])),
            }
        }
        ("GET", ["artists", urn, "star"]) => {
            let _ = urn;
            ok(json!({ "star": false, "aura_id": null, "custom_hex": null }))
        }
        ("GET", ["artists", urn]) => {
            let id = id_of(urn);
            match s.sc_get(&format!("/users/{id}"), token.as_deref()).await {
                Ok((status, v)) => json_resp(status, &normalize_urn(v)),
                Err(e) => err(502, &e),
            }
        }

        // ── search ──────────────────────────────────────────────────
        ("GET", ["search", "db", kind]) => {
            let kind: &str = *kind;
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            let query = q_str(&q, "q").unwrap_or_default();
            let user_urn = q_str(&q, "user_urn");

            if let Some(u) = user_urn.as_deref() {
                let id = id_of(u);
                let path = match kind {
                    "tracks" => format!("/users/{id}/tracks?limit={limit}&offset={offset}"),
                    "playlists" => format!("/users/{id}/playlists?limit={limit}&offset={offset}"),
                    _ => String::new(),
                };
                if !path.is_empty() {
                    if let Ok((_s, v)) = s.sc_get(&path, token.as_deref()).await {
                        let items = sc_items(&v).into_iter().map(normalize_urn).collect();
                        return Ok(ok(page(items, page_no, limit, sc_has_more(&v))));
                    }
                }
                return Ok(ok(empty_page(page_no, limit)));
            }

            let filter = match kind {
                "tracks" => "track",
                "playlists" | "albums" => "playlist",
                "users" => "user",
                "artists" => return Ok(ok(empty_page(page_no, limit))),
                _ => "track",
            };
            let path = format!(
                "/search?q={}&filter={filter}&limit={limit}&offset={offset}",
                urlencoding::encode(&query)
            );
            match s.sc_get(&path, token.as_deref()).await {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let mut items: Vec<Value> =
                        sc_items(&v).into_iter().map(normalize_urn).collect();
                    if kind == "albums" {
                        items.retain(|p| {
                            p.get("is_album").and_then(Value::as_bool).unwrap_or(false)
                                || matches!(
                                    p.get("kind").and_then(Value::as_str),
                                    Some("album") | Some("ep") | Some("single") | Some("compilation")
                                )
                        });
                        if items.is_empty() {
                            // Fall back to playlists so the tab is not empty.
                            items = sc_items(&v).into_iter().map(normalize_urn).collect();
                        }
                    }
                    ok(page(items, page_no, limit, sc_has_more(&v)))
                }
                Ok((_status, _v)) => ok(empty_page(page_no, limit)),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["search", "vibe"]) | ("GET", ["search", "lyrics"]) => {
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            ok(empty_page(page_no, limit))
        }

        // ── featured / home ─────────────────────────────────────────
        ("GET", ["featured"]) => {
            if let Ok((status, v)) = s
                .sc_get("/featured_tracks/all?limit=1", token.as_deref())
                .await
            {
                if (200..300).contains(&status) {
                    if let Some(first) = sc_items(&v).into_iter().next() {
                        let kind = first
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or("track");
                        let ty = if kind.contains("playlist") { "playlist" } else { "track" };
                        return Ok(ok(json!({ "type": ty, "data": normalize_urn(first) })));
                    }
                }
            }
            ok(Value::Null)
        }

        // ── discover (catalog stubs) ────────────────────────────────
        ("GET", ["discover", "summary"]) => ok(json!({
            "artists_count": 0,
            "albums_count": 0,
            "fresh_count": 0,
            "fresh_window_days": 7,
        })),
        ("GET", ["discover", _rest @ ..]) => ok(json!({ "items": [], "next_cursor": null })),

        // ── recommendations (stubs) ─────────────────────────────────
        ("POST", ["recommendations", "feedback"])
        | ("POST", ["recommendations", "wave", "feedback"]) => ok(json!({ "ok": true, "cursor": null })),
        ("GET", ["recommendations", "wave"]) | ("GET", ["recommendations", "wave", ..]) => {
            ok(json!({ "tracks": [], "cursor": "" }))
        }
        ("GET", ["recommendations", "similar", _id]) => {
            let limit = q_u64(&q, "limit", 10);
            ok(empty_page(0, limit))
        }
        ("GET", ["recommendations", _rest @ ..]) => ok(empty_page(0, 30)),

        // ── history ─────────────────────────────────────────────────
        ("GET", ["history"]) => {
            let limit = q_u64(&q, "limit", 50) as usize;
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
            ok(json!({ "collection": slice, "total": total }))
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

        // ── streaming/storage fallbacks (anon path handles audio) ───
        ("GET", ["stream", ..]) | ("GET", ["download", ..]) | ("GET", ["storage", ..])
        | ("GET", ["redirect", ..]) => err(404, "direct mode: use anon path"),

        _ => err(404, "not found"),
    };

    Ok(resp)
}

// ─── shared data helpers ─────────────────────────────────────────────────

fn stream_to_tracks(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(items) = v.get("collection").and_then(Value::as_array) {
        for it in items {
            let track = it
                .get("origin")
                .filter(|o| o.get("kind").and_then(Value::as_str) == Some("track"))
                .cloned()
                .or_else(|| it.get("track").cloned())
                .or_else(|| {
                    it.get("origin")
                        .filter(|o| o.get("kind").and_then(Value::as_str).is_none())
                        .cloned()
                });
            if let Some(t) = track {
                if !t.is_null() {
                    out.push(normalize_urn(t));
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| urn_of(t).map(|u| seen.insert(u)).unwrap_or(false));
    out
}

async fn merge_local_likes(state: &DirectState, mut items: Vec<Value>, page_no: u64) -> Vec<Value> {
    let store = state.store.lock().await;
    items.retain(|t| {
        urn_of(t)
            .map(|u| !store.is_unliked(&u))
            .unwrap_or(true)
    });
    if page_no == 0 {
        let mut extra: Vec<Value> = store
            .liked_tracks
            .iter()
            .filter(|t| urn_of(t).map(|u| !items.iter().any(|i| urn_of(i).as_deref() == Some(u.as_str()))).unwrap_or(false))
            .cloned()
            .collect();
        extra.extend(items);
        items = extra;
    }
    items
}

async fn merge_local_playlist_likes(
    state: &DirectState,
    mut items: Vec<Value>,
    page_no: u64,
) -> Vec<Value> {
    let store = state.store.lock().await;
    items.retain(|p| {
        urn_of(p)
            .map(|u| !store.is_playlist_unliked(&u))
            .unwrap_or(true)
    });
    if page_no == 0 {
        let mut extra: Vec<Value> = store
            .liked_playlists
            .iter()
            .filter(|p| urn_of(p).map(|u| !items.iter().any(|i| urn_of(i).as_deref() == Some(u.as_str()))).unwrap_or(false))
            .cloned()
            .collect();
        extra.extend(items);
        items = extra;
    }
    items
}
