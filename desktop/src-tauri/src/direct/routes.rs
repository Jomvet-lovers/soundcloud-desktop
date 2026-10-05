//! Local HTTP API implementing the subset of the app backend that the
//! frontend uses. Reads are resolved against SoundCloud api-v2; writes are
//! persisted locally in [`super::store::LocalStore`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::{json, Value};
use tauri::Manager;
use warp::http::{HeaderMap, Method, StatusCode};
use warp::reply::{Reply, Response};
use warp::{path::FullPath, Filter};

use super::sc::{fetch_me, id_of, SC_API};
use super::webview::{emit_sync_error, spawn_write, spawn_write_silent};
use super::DirectState;

pub async fn start(state: Arc<DirectState>) -> u16 {
    let routes = warp::any()
        .and(warp::method())
        .and(warp::path::full())
        // `query::raw()` rejects requests without a query string, so default
        // to an empty one instead of failing every GET without params.
        .and(
            warp::query::raw()
                .or(warp::any().map(String::new))
                .unify(),
        )
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

/// Normalizes an item (and nested selection items) for the frontend:
/// - ensures a `urn` (playlists/users without one get it from `id`/`kind`),
/// - mirrors `calculated_artwork_url` / `avatar_url` into `artwork_url`,
/// - mirrors `artwork_url` into `cover_url`.
fn normalize_deep(mut item: Value) -> Value {
    item = normalize_urn(item);
    if item.get("artwork_url").map(Value::is_null).unwrap_or(true) {
        let fallback = item
            .get("calculated_artwork_url")
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| item.get("avatar_url").filter(|v| !v.is_null()).cloned());
        if let Some(art) = fallback {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("artwork_url".into(), art);
            }
        }
    }
    if item.get("cover_url").is_none() {
        if let Some(art) = item.get("artwork_url").cloned() {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("cover_url".into(), art);
            }
        }
    }
    if let Some(collection) = item
        .get_mut("items")
        .and_then(|i| i.get_mut("collection"))
        .and_then(Value::as_array_mut)
    {
        for entry in collection.iter_mut() {
            let taken = entry.take();
            *entry = normalize_deep(taken);
        }
    }
    item
}

/// Replaces id-only track stubs (removed/unavailable SoundCloud tracks come
/// back without `urn`/`media`) with full track objects. Unresolvable stubs are
/// dropped so the frontend never renders empty rows.
async fn hydrate_track_stubs(state: &DirectState, token: Option<&str>, tracks: &mut Vec<Value>) {
    // A stub is any entry without a title: the raw `{id}` refs from SC
    // playlists, and the `{urn,id}` placeholders older reorder code wrote.
    let ids: Vec<String> = tracks
        .iter()
        .filter(|t| t.get("title").is_none())
        .filter_map(|t| {
            t.get("id")
                .map(|i| i.to_string().trim_matches('"').to_string())
                .or_else(|| t.get("urn").and_then(Value::as_str).map(id_of))
        })
        .collect();
    if ids.is_empty() {
        return;
    }
    let mut by_id: HashMap<String, Value> = HashMap::new();
    for chunk in ids.chunks(50) {
        let path = format!("/tracks?ids={}", chunk.join(","));
        if let Ok((st, hv)) = state.sc_get(&path, token).await {
            if (200..300).contains(&st) {
                if let Some(arr) = hv.as_array() {
                    for t in arr {
                        if let Some(id) = t
                            .get("id")
                            .map(|i| i.to_string().trim_matches('"').to_string())
                        {
                            by_id.insert(id, normalize_urn(t.clone()));
                        }
                    }
                }
            }
        }
    }
    tracks.retain_mut(|t| {
        if t.get("title").is_some() {
            return true;
        }
        let id = t
            .get("id")
            .map(|i| i.to_string().trim_matches('"').to_string())
            .or_else(|| t.get("urn").and_then(Value::as_str).map(id_of));
        match id {
            Some(id) => match by_id.remove(&id) {
                Some(full) => {
                    *t = full;
                    true
                }
                None => false,
            },
            None => false,
        }
    });
}

/// Local playlist copy with stubs hydrated and (for SC-owned playlists) any
/// missing metadata refilled from the SoundCloud detail. The repair is
/// persisted, but a partial fetch never drops stored tracks.
async fn repaired_local_playlist(
    state: &DirectState,
    token: Option<&str>,
    urn: &str,
) -> Option<Value> {
    let local = {
        let store = state.store.lock().await;
        store.find_playlist(urn)
    }?;
    let mut repaired = local;
    let mut changed = false;

    let mut tracks = repaired
        .get("tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let before = tracks.clone();
    hydrate_track_stubs(state, token, &mut tracks).await;
    if tracks != before && tracks.len() == before.len() {
        if let Some(obj) = repaired.as_object_mut() {
            obj.insert("track_count".into(), json!(tracks.len()));
            obj.insert("tracks".into(), json!(tracks));
        }
        changed = true;
    }

    // The old reorder fallback could create a bare entry without a title or
    // artwork; refill those from the SoundCloud detail.
    if urn.starts_with("soundcloud:playlists:") && repaired.get("title").is_none() {
        let id = id_of(urn);
        if let Ok((status, detail)) = state.sc_get(&format!("/playlists/{id}"), token).await {
            if (200..300).contains(&status) {
                if let (Some(dst), Some(src)) = (repaired.as_object_mut(), detail.as_object()) {
                    for key in [
                        "title",
                        "description",
                        "artwork_url",
                        "user",
                        "genre",
                        "created_at",
                        "sharing",
                        "permalink_url",
                        "duration",
                    ] {
                        let empty = dst.get(key).map(Value::is_null).unwrap_or(true);
                        if empty {
                            if let Some(val) = src.get(key) {
                                dst.insert(key.to_string(), val.clone());
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if changed {
        state.store.lock().await.upsert_playlist(repaired.clone());
    }
    Some(repaired)
}

/// Local ordering for search results (SoundCloud ignores `sort`).
fn sort_tracks(items: &mut [Value], sort: &str) {
    let num = |v: &Value, keys: &[&str]| -> i64 {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(Value::as_i64))
            .unwrap_or(0)
    };
    match sort {
        "plays" => items.sort_by(|a, b| {
            num(b, &["playback_count"]).cmp(&num(a, &["playback_count"]))
        }),
        "likes" => items.sort_by(|a, b| {
            num(b, &["favoritings_count", "likes_count"])
                .cmp(&num(a, &["favoritings_count", "likes_count"]))
        }),
        "newest" => items.sort_by(|a, b| {
            let ts = |v: &Value| {
                v.get("created_at")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            ts(b).cmp(&ts(a))
        }),
        _ => {}
    }
}

/// Raw SoundCloud playlist -> the `AlbumDetail` shape the frontend renders.
fn album_detail(v: &Value) -> Value {
    let id = v
        .get("id")
        .map(|x| x.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let is_album = v.get("is_album").and_then(Value::as_bool).unwrap_or(false);
    let set_type = v.get("set_type").and_then(Value::as_str).unwrap_or("");
    let ty = if !set_type.is_empty() {
        set_type
    } else if is_album {
        "album"
    } else {
        "playlist"
    };
    let release_year = v
        .get("release_date")
        .and_then(Value::as_str)
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<u64>().ok());
    let primary = v.get("user").and_then(|u| {
        let uid = u
            .get("id")
            .map(|x| x.to_string().trim_matches('"').to_string())?;
        Some(json!({
            "id": uid,
            "name": u.get("username").cloned().unwrap_or(Value::Null),
            "role": "primary",
            "avatar_url": u.get("avatar_url").cloned().unwrap_or(Value::Null),
        }))
    });
    let artists = match &primary {
        Some(p) => json!([p.clone()]),
        None => json!([]),
    };
    let tracks: Vec<Value> = v
        .get("tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(normalize_urn)
        .collect();
    json!({
        "id": id,
        "title": v.get("title").cloned().unwrap_or(Value::Null),
        "type": ty,
        "release_year": release_year,
        "cover_url": v.get("artwork_url").cloned().unwrap_or(Value::Null),
        "confidence": 1.0,
        "primary_artist": primary,
        "artists": artists,
        "tracks": tracks,
    })
}

/// Raw SoundCloud playlist -> the `ArtistAlbum` list item shape.
fn artist_album(p: &Value) -> Value {
    let id = p
        .get("id")
        .map(|x| x.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let is_album = p.get("is_album").and_then(Value::as_bool).unwrap_or(false);
    let set_type = p.get("set_type").and_then(Value::as_str).unwrap_or("");
    let ty = if !set_type.is_empty() {
        set_type
    } else if is_album {
        "album"
    } else {
        "playlist"
    };
    let release_year = p
        .get("release_date")
        .and_then(Value::as_str)
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<u64>().ok());
    json!({
        "id": id,
        "title": p.get("title").cloned().unwrap_or(Value::Null),
        "type": ty,
        "release_year": release_year,
        "cover_url": p.get("artwork_url").cloned().unwrap_or(Value::Null),
        "role": "primary",
    })
}

/// Raw SoundCloud user -> the `ArtistDetail` shape the frontend renders.
fn artist_detail(v: &Value, socials: Vec<Value>) -> Value {
    let id = v
        .get("id")
        .map(|x| x.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let track_count = v.get("track_count").and_then(Value::as_u64).unwrap_or(0);
    let playlist_count = v
        .get("playlist_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "id": id,
        "name": v.get("username").cloned().unwrap_or(Value::Null),
        "country": v.get("country_code").cloned().unwrap_or(Value::Null),
        "bio": v.get("description").cloned().unwrap_or(Value::Null),
        "avatar_url": v.get("avatar_url").cloned().unwrap_or(Value::Null),
        "confidence": 1.0,
        "socials": socials,
        "sc_accounts": [],
        "track_count": track_count,
        "track_count_primary": track_count,
        "track_count_featured": 0,
        "album_count": playlist_count,
        "popular_tracks": [],
        "related_artists": [],
    })
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

async fn liked_track_ids(state: &DirectState, token: &str) -> Vec<String> {
    state
        .sc_get("/me/track_likes/ids", Some(token))
        .await
        .ok()
        .map(|(_, v)| {
            v.get("collection")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_u64().map(|n| n.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default()
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
            drop(store);
            // Best-effort sync. SoundCloud's web client adds a signed query
            // param here; without it the server may reject the call, in which
            // case the local follow still stands.
            let id = id_of(urn);
            if let Some(t) = token.as_deref() {
                if let Ok(cid) = s.client_id().await {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "POST",
                        format!("{SC_API}/me/followings/{id}?client_id={cid}"),
                        None,
                    );
                }
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
            drop(store);
            let id = id_of(urn);
            if let Some(t) = token.as_deref() {
                if let Ok(cid) = s.client_id().await {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "DELETE",
                        format!("{SC_API}/me/followings/{id}?client_id={cid}"),
                        None,
                    );
                }
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
        ("PUT", ["likes", "tracks", urn]) | ("POST", ["likes", "tracks", urn]) => {
            let id = id_of(urn);
            let track = match token.as_deref() {
                Some(t) => s
                    .sc_get_opt(&format!("/tracks/{id}"), Some(t))
                    .await
                    .unwrap_or_else(|| json!({ "urn": urn, "id": id.parse::<u64>().unwrap_or(0) })),
                None => json!({ "urn": urn, "id": id.parse::<u64>().unwrap_or(0) }),
            };
            s.store.lock().await.like_track(normalize_urn(track));
            // Best-effort SoundCloud sync (DataDome-protected; via writer webview).
            if let Some(t) = token.as_deref() {
                if let (Ok(my), Ok(cid)) = (s.my_user_id(t).await, s.client_id().await) {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "PUT",
                        format!("{SC_API}/users/{my}/track_likes/{id}?client_id={cid}"),
                        None,
                    );
                }
            }
            ok(json!({ "ok": true }))
        }
        ("DELETE", ["likes", "tracks", urn]) => {
            s.store.lock().await.unlike_track(urn);
            let id = id_of(urn);
            if let Some(t) = token.as_deref() {
                if let (Ok(my), Ok(cid)) = (s.my_user_id(t).await, s.client_id().await) {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "DELETE",
                        format!("{SC_API}/users/{my}/track_likes/{id}?client_id={cid}"),
                        None,
                    );
                }
            }
            ok(json!({ "ok": true }))
        }

        ("GET", ["likes", "playlists", urn]) => {
            let store = s.store.lock().await;
            ok(json!({ "liked": store.is_playlist_liked(urn) }))
        }
        ("PUT", ["likes", "playlists", urn]) | ("POST", ["likes", "playlists", urn]) => {
            let id = id_of(urn);
            let playlist = match token.as_deref() {
                Some(t) => s
                    .sc_get_opt(&format!("/playlists/{id}"), Some(t))
                    .await
                    .unwrap_or_else(|| json!({ "urn": urn })),
                None => json!({ "urn": urn }),
            };
            s.store.lock().await.like_playlist(normalize_urn(playlist));
            if let Some(t) = token.as_deref() {
                if let (Ok(my), Ok(cid)) = (s.my_user_id(t).await, s.client_id().await) {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "PUT",
                        format!("{SC_API}/users/{my}/playlist_likes/{id}?client_id={cid}"),
                        None,
                    );
                }
            }
            ok(json!({ "ok": true }))
        }
        ("DELETE", ["likes", "playlists", urn]) => {
            s.store.lock().await.unlike_playlist(urn);
            let id = id_of(urn);
            if let Some(t) = token.as_deref() {
                if let (Ok(my), Ok(cid)) = (s.my_user_id(t).await, s.client_id().await) {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "DELETE",
                        format!("{SC_API}/users/{my}/playlist_likes/{id}?client_id={cid}"),
                        None,
                    );
                }
            }
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
            drop(store);
            ok(entry)
        }
        ("GET", ["tracks", _urn, "sharing"]) => ok(json!({ "sharing": "public" })),
        ("PUT", ["tracks", _urn, "sharing"]) | ("DELETE", ["tracks", _urn, "sharing"]) => {
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
            let title = input
                .get("title")
                .cloned()
                .unwrap_or(json!("Untitled playlist"));
            let sharing = input.get("sharing").cloned().unwrap_or(json!("private"));
            let track_ids: Vec<u64> = input
                .get("tracks")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|r| r.get("urn").and_then(Value::as_str))
                        .filter_map(|u| id_of(u).parse::<u64>().ok())
                        .collect()
                })
                .unwrap_or_default();
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
            let mut store = s.store.lock().await;
            let urn = store.next_local_playlist_urn();
            let playlist = json!({
                "id": urn.rsplit(':').next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0),
                "urn": urn.clone(),
                "title": title.clone(),
                "description": input.get("description").cloned().unwrap_or(Value::Null),
                "genre": input.get("genre").cloned().unwrap_or(json!("")),
                "tag_list": input.get("tag_list").cloned().unwrap_or(json!("")),
                "sharing": sharing.clone(),
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
            drop(store);

            if let Some(t) = token.as_deref() {
                if let Ok(cid) = s.client_id().await {
                    let payload = json!({
                        "playlist": {
                            "title": title,
                            "sharing": sharing,
                            "tracks": track_ids,
                        }
                    });
                    let url = format!("{SC_API}/playlists?client_id={cid}");
                    let extract = "(function(s){try{return String((JSON.parse(s)||{}).id||'');}catch(e){return '';}})";
                    match super::webview::execute_with(
                        &s.app,
                        Some(t),
                        "POST",
                        &url,
                        Some(&payload),
                        Some(extract),
                        true,
                    )
                    .await
                    {
                        Ok(o) if (200..300).contains(&o.status) => {
                            if let Ok(id) = o.payload.trim().parse::<u64>() {
                                let detail = match s.sc_get_opt(&format!("/playlists/{id}"), Some(t)).await
                                {
                                    Some(v) => normalize_urn(v),
                                    None => {
                                        let mut p = playlist.clone();
                                        if let Some(obj) = p.as_object_mut() {
                                            obj.insert("id".into(), json!(id));
                                            obj.insert(
                                                "urn".into(),
                                                json!(format!("soundcloud:playlists:{id}")),
                                            );
                                            obj.insert("local".into(), json!(false));
                                        }
                                        p
                                    }
                                };
                                s.store.lock().await.rekey_playlist(&urn, detail.clone());
                                return Ok(ok(detail));
                            }
                        }
                        Ok(o) => {
                            emit_sync_error(
                                &s.app,
                                "POST",
                                &url,
                                o.status,
                                o.captcha.is_some(),
                                None,
                            );
                        }
                        Err(e) => emit_sync_error(&s.app, "POST", &url, 0, false, Some(&e)),
                    }
                }
            }
            ok(playlist)
        }
        ("GET", ["playlists", urn]) => {
            {
                let store = s.store.lock().await;
                if store.is_deleted_playlist(urn) {
                    return Ok(err(404, "playlist deleted"));
                }
            }
            if let Some(local) = repaired_local_playlist(s, token.as_deref(), urn).await {
                return Ok(ok(local));
            }
            let id = id_of(urn);
            match s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await {
                Ok((status, mut v)) if (200..300).contains(&status) => {
                    if let Some(slot) = v.get_mut("tracks").and_then(Value::as_array_mut) {
                        let mut list = std::mem::take(slot);
                        hydrate_track_stubs(s, token.as_deref(), &mut list).await;
                        *slot = list;
                    }
                    json_resp(status, &normalize_urn(v))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["playlists", urn, "tracks"]) => {
            let limit = q_u64(&q, "limit", 200);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            let local = repaired_local_playlist(s, token.as_deref(), urn).await;
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
            // SoundCloud dropped `/playlists/{id}/tracks`; the playlist detail
            // already carries the full track objects, so paginate those.
            match s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await {
                Ok((_status, v)) => {
                    let mut all = v
                        .get("tracks")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    hydrate_track_stubs(s, token.as_deref(), &mut all).await;
                    let total = all.len() as u64;
                    let slice: Vec<Value> = all
                        .into_iter()
                        .skip(offset as usize)
                        .take(limit as usize)
                        .map(normalize_urn)
                        .collect();
                    let has_more = offset + (slice.len() as u64) < total;
                    ok(page(slice, page_no, limit, has_more))
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
                        if let Some(u) = r.get("urn").and_then(Value::as_str) {
                            return store_likes
                                .iter()
                                .find(|t| t.get("urn").and_then(Value::as_str) == Some(u))
                                .cloned();
                        }
                        let id = r
                            .get("id")
                            .or_else(|| r.get("track_id"))
                            .map(|v| v.to_string().trim_matches('"').to_string());
                        id.and_then(|id| {
                            store_likes
                                .iter()
                                .find(|t| {
                                    t.get("id")
                                        .map(|v| v.to_string().trim_matches('"') == id.as_str())
                                        .unwrap_or(false)
                                })
                                .cloned()
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
        ("POST", ["playlists", urn, "tracks"]) => {
            let b = body_json(&body);
            let add = b.get("add").and_then(Value::as_str).map(str::to_string);
            let order: Option<Vec<String>> = b
                .get("order")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                });

            // Resolve the added track before locking the store.
            let fetched = match add.as_deref() {
                Some(u) => {
                    let id = id_of(u);
                    let resolved = match token.as_deref() {
                        Some(t) => s.sc_get_opt(&format!("/tracks/{id}"), Some(t)).await,
                        None => None,
                    };
                    resolved
                        .map(normalize_urn)
                        .or_else(|| Some(json!({ "urn": u, "id": id.parse::<u64>().unwrap_or(0) })))
                }
                None => None,
            };

            let is_sc = urn.starts_with("soundcloud:playlists:");
            // Base: the hydrated local copy when present; otherwise the full
            // playlist detail for SC-owned playlists (SC dropped
            // `/playlists/{id}/tracks`, and the detail also carries the
            // metadata the local copy needs).
            let local = repaired_local_playlist(s, token.as_deref(), urn).await;
            let mut playlist = local.clone();
            let mut tracks: Vec<Value> = local
                .as_ref()
                .and_then(|lp| lp.get("tracks").and_then(Value::as_array).cloned())
                .unwrap_or_default();
            if playlist.is_none() && is_sc {
                let id = id_of(urn);
                if let Ok((status, v)) = s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await
                {
                    if (200..300).contains(&status) {
                        let detail = normalize_urn(v);
                        if let Some(list) = detail.get("tracks").and_then(Value::as_array) {
                            let mut list = list.clone();
                            hydrate_track_stubs(s, token.as_deref(), &mut list).await;
                            tracks = list.into_iter().map(normalize_urn).collect();
                        }
                        playlist = Some(detail);
                    }
                }
            }

            if let Some(track) = fetched {
                let t_urn = urn_of(&track).unwrap_or_default();
                if !tracks
                    .iter()
                    .any(|t| urn_of(t).as_deref() == Some(t_urn.as_str()))
                {
                    tracks.push(track);
                }
            }

            if let Some(order) = order {
                let mut ordered: Vec<Value> = Vec::new();
                for u in &order {
                    if let Some(t) = tracks.iter().find(|t| urn_of(t).as_deref() == Some(u.as_str())) {
                        ordered.push(t.clone());
                    } else {
                        // Unknown urn: resolve the full track instead of
                        // persisting a data-less stub.
                        let id = id_of(u);
                        let fetched = match token.as_deref() {
                            Some(t) => s.sc_get_opt(&format!("/tracks/{id}"), Some(t)).await,
                            None => None,
                        };
                        ordered.push(fetched.map(normalize_urn).unwrap_or_else(|| {
                            json!({ "urn": u, "id": id.parse::<u64>().unwrap_or(0) })
                        }));
                    }
                }
                tracks = ordered;
            }

            let mut playlist = playlist.unwrap_or_else(|| {
                json!({ "urn": urn, "tracks": [], "track_count": 0, "local": !is_sc })
            });
            if let Some(obj) = playlist.as_object_mut() {
                obj.insert("track_count".into(), json!(tracks.len()));
                obj.insert("tracks".into(), json!(tracks));
            }
            s.store.lock().await.upsert_playlist(playlist.clone());

            if is_sc {
                if let (Some(t), Ok(cid)) = (token.as_deref(), s.client_id().await) {
                    let ids: Vec<Value> = tracks
                        .iter()
                        .filter_map(urn_of)
                        .filter_map(|u| {
                            let id = id_of(&u);
                            id.parse::<i64>().ok().map(|n| json!(n))
                        })
                        .collect();
                    let payload = json!({ "playlist": { "tracks": ids } });
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "PUT",
                        format!("{SC_API}/playlists/{}?client_id={cid}", id_of(urn)),
                        Some(payload),
                    );
                }
            }
            ok(playlist)
        }
        ("DELETE", ["playlists", urn]) => {
            s.store.lock().await.delete_playlist(urn);
            if urn.starts_with("soundcloud:playlists:") {
                if let (Some(t), Ok(cid)) = (token.as_deref(), s.client_id().await) {
                    spawn_write(
                        s.app.clone(),
                        t.to_string(),
                        "DELETE",
                        format!("{SC_API}/playlists/{}?client_id={cid}", id_of(urn)),
                        None,
                    );
                }
            }
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
            // SoundCloud's /web-profiles requires the urn form
            // (`soundcloud:users:<id>`), not the bare numeric id.
            let sc_id = if urn.contains(':') {
                urn.to_string()
            } else {
                format!("soundcloud:users:{urn}")
            };
            match s
                .sc_get(&format!("/users/{sc_id}/web-profiles"), token.as_deref())
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    // SC items use `network`; the frontend expects `service`.
                    let items: Vec<Value> = v
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let url = p
                                .get("url")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            json!({
                                "id": if url.is_empty() { format!("wp-{i}") } else { url.clone() },
                                "kind": "web-profile",
                                "service": p
                                    .get("network")
                                    .or_else(|| p.get("service"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("link"),
                                "title": p.get("title").and_then(Value::as_str).unwrap_or(""),
                                "url": url,
                                "username": p.get("username").cloned().unwrap_or(Value::Null),
                            })
                        })
                        .collect();
                    ok(json!(items))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["users", _urn, "subscription"]) => ok(json!({ "premium": false })),
        ("GET", ["users", _urn, "aura"]) => ok(json!({ "aura_id": null, "custom_hex": null })),
        ("GET", ["users", my_urn, "followings", target_urn]) => {
            let target_id = id_of(target_urn);
            {
                let store = s.store.lock().await;
                if store.followed.iter().any(|u| u == target_urn) {
                    return Ok(ok(json!(true)));
                }
                if store.unfollowed.iter().any(|u| u == target_urn) {
                    return Ok(ok(json!(false)));
                }
            }
            let my_id = id_of(my_urn);
            let mut following = false;
            let limit = 200u64;
            let mut offset = 0u64;
            loop {
                match s
                    .sc_get(
                        &format!("/users/{my_id}/followings?limit={limit}&offset={offset}"),
                        token.as_deref(),
                    )
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => {
                        let hit = sc_items(&v).iter().any(|u| {
                            u.get("id")
                                .and_then(Value::as_u64)
                                .map(|i| i.to_string())
                                == Some(target_id.clone())
                        });
                        if hit {
                            following = true;
                            break;
                        }
                        if !sc_has_more(&v) || offset >= 1000 {
                            break;
                        }
                        offset += limit;
                    }
                    _ => break,
                }
            }
            ok(json!(following))
        }
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
        ("GET", ["users", urn, "likes", "tracks"]) => {
            let id = id_of(urn);
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            // SoundCloud's track_likes is cursor-paged: a numeric `offset`
            // is rejected ("invalid cursor format"). The opaque cursor comes
            // from the previous page's `next_href`, so pass it through.
            let cursor = q_str(&q, "cursor");
            let path = match cursor.as_deref() {
                Some(c) if c.starts_with(SC_API) => c[SC_API.len()..].to_string(),
                _ => format!("/users/{id}/track_likes?limit={limit}&offset=0"),
            };
            match s.sc_get(&path, token.as_deref()).await {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let next_cursor = v
                        .get("next_href")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let items: Vec<Value> = sc_items(&v)
                        .into_iter()
                        .filter_map(|it| it.get("track").cloned())
                        .filter(|t| !t.is_null())
                        .map(normalize_urn)
                        .collect();
                    let items = if cursor.is_none() {
                        merge_local_likes(s, items, 0).await
                    } else {
                        items
                    };
                    let mut body = page(items, page_no, limit, next_cursor.is_some());
                    if let Some(nc) = next_cursor {
                        body["next_cursor"] = json!(nc);
                    }
                    ok(body)
                }
                _ => ok(empty_page(page_no, limit)),
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

        // ── albums / artists ────────────────────────────────────────
        ("GET", ["albums", urn]) => {
            let id = id_of(urn);
            match s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await {
                Ok((status, mut v)) if (200..300).contains(&status) => {
                    if let Some(slot) = v.get_mut("tracks").and_then(Value::as_array_mut) {
                        let mut list = std::mem::take(slot);
                        hydrate_track_stubs(s, token.as_deref(), &mut list).await;
                        *slot = list;
                    }
                    ok(album_detail(&v))
                }
                Ok((status, v)) => json_resp(status, &v),
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
                    let items: Vec<Value> =
                        sc_items(&v).into_iter().map(|p| artist_album(&p)).collect();
                    ok(json!(items))
                }
                Err(_) => ok(json!([])),
            }
        }
        ("GET", ["artists", urn, "tracks"]) => {
            let id = id_of(urn);
            let role = q_str(&q, "role").unwrap_or_else(|| "primary".to_string());
            let limit = q_u64(&q, "limit", 80);
            let offset = q_u64(&q, "offset", 0);
            let filter = if role == "featured" {
                "&filter=featured"
            } else {
                ""
            };
            match s
                .sc_get(
                    &format!("/users/{id}/tracks?limit={limit}&offset={offset}{filter}"),
                    token.as_deref(),
                )
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items: Vec<Value> =
                        sc_items(&v).into_iter().map(normalize_urn).collect();
                    ok(json!({ "collection": items }))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        ("GET", ["artists", urn, "related"]) => {
            let id = id_of(urn);
            let mut counts: HashMap<u64, (Value, u32)> = HashMap::new();
            if let Ok((status, v)) = s
                .sc_get(
                    &format!("/users/{id}/tracks?limit=3&offset=0"),
                    token.as_deref(),
                )
                .await
            {
                if (200..300).contains(&status) {
                    for track in sc_items(&v).into_iter().take(3) {
                        let tid = track
                            .get("id")
                            .map(|x| x.to_string().trim_matches('"').to_string());
                        let Some(tid) = tid else { continue };
                        let Ok((st, rv)) = s
                            .sc_get(
                                &format!("/tracks/{tid}/related?limit=12"),
                                token.as_deref(),
                            )
                            .await
                        else {
                            continue;
                        };
                        if !(200..300).contains(&st) {
                            continue;
                        }
                        for rel in sc_items(&rv) {
                            if let Some(user) = rel.get("user") {
                                if let Some(uid) = user.get("id").and_then(Value::as_u64) {
                                    if uid.to_string() == id {
                                        continue;
                                    }
                                    let entry =
                                        counts.entry(uid).or_insert_with(|| (user.clone(), 0));
                                    entry.1 += 1;
                                }
                            }
                        }
                    }
                }
            }
            let mut list: Vec<(u64, Value, u32)> =
                counts.into_iter().map(|(k, (u, c))| (k, u, c)).collect();
            list.sort_by(|a, b| b.2.cmp(&a.2));
            let out: Vec<Value> = list
                .into_iter()
                .take(8)
                .map(|(uid, u, c)| {
                    json!({
                        "id": uid.to_string(),
                        "name": u.get("username").cloned().unwrap_or(Value::Null),
                        "country": u.get("country_code").cloned().unwrap_or(Value::Null),
                        "avatar_url": u.get("avatar_url").cloned().unwrap_or(Value::Null),
                        "weight": c,
                    })
                })
                .collect();
            ok(json!(out))
        }
        ("GET", ["artists", urn, "star"]) => {
            let _ = urn;
            ok(json!({ "star": false, "aura_id": null, "custom_hex": null }))
        }
        ("GET", ["artists", urn]) => {
            let id = id_of(urn);
            match s.sc_get(&format!("/users/{id}"), token.as_deref()).await {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let profiles = s
                        .sc_get_opt(&format!("/users/{id}/web-profiles"), token.as_deref())
                        .await;
                    let socials: Vec<Value> = profiles
                        .as_ref()
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|p| {
                            let url = p.get("url").and_then(Value::as_str)?;
                            Some(json!({
                                "kind": p
                                    .get("network")
                                    .or_else(|| p.get("service"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("link"),
                                "url": url,
                                "source": "sc",
                                "verified": false,
                            }))
                        })
                        .collect();
                    ok(artist_detail(&v, socials))
                }
                Ok((status, v)) => json_resp(status, &v),
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

            // SoundCloud's search ignores `sort`, so the non-relevance orders
            // fetch one wide window, sort it locally and paginate from the
            // cached result.
            let sort = q_str(&q, "sort").unwrap_or_else(|| "relevance".to_string());
            if kind == "tracks" && user_urn.is_none() && sort != "relevance" && !query.is_empty() {
                let cache_key = format!("{query}\u{1}{sort}");
                let cached = {
                    let cache = s.search_cache.lock().await;
                    cache.get(&cache_key).cloned()
                };
                let all = match cached {
                    Some((at, items)) if at.elapsed() < Duration::from_secs(120) => items,
                    _ => {
                        let path = format!(
                            "/search/tracks?q={}&limit=200&offset=0",
                            urlencoding::encode(&query)
                        );
                        let items = match s.sc_get(&path, token.as_deref()).await {
                            Ok((st, v)) if (200..300).contains(&st) => {
                                let mut items: Vec<Value> = sc_items(&v)
                                    .into_iter()
                                    .map(normalize_urn)
                                    .map(normalize_deep)
                                    .collect();
                                sort_tracks(&mut items, &sort);
                                items
                            }
                            _ => Vec::new(),
                        };
                        let mut cache = s.search_cache.lock().await;
                        if cache.len() > 24 {
                            cache.clear();
                        }
                        cache.insert(cache_key, (Instant::now(), items.clone()));
                        items
                    }
                };
                let total = all.len() as u64;
                let slice: Vec<Value> = all
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect();
                let has_more = (offset + slice.len() as u64) < total;
                return Ok(ok(page(slice, page_no, limit, has_more)));
            }

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

            let endpoint = match kind {
                "tracks" => "tracks",
                "playlists" => "playlists",
                "users" => "users",
                "albums" => "albums",
                "artists" => return Ok(ok(empty_page(page_no, limit))),
                _ => "tracks",
            };
            let path = format!(
                "/search/{endpoint}?q={}&limit={limit}&offset={offset}",
                urlencoding::encode(&query)
            );
            match s.sc_get(&path, token.as_deref()).await {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items: Vec<Value> = sc_items(&v)
                        .into_iter()
                        .map(normalize_urn)
                        .map(normalize_deep)
                        .collect();
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
        // SoundCloud "Made for you" / "Discover with Stations" selections.
        ("GET", ["discover", "mixed"]) => {
            match s
                .sc_get("/mixed-selections?limit=20", token.as_deref())
                .await
            {
                Ok((status, v)) if (200..300).contains(&status) => {
                    let items: Vec<Value> =
                        sc_items(&v).into_iter().map(normalize_deep).collect();
                    ok(json!({ "collection": items }))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
        // SoundCloud system playlists (Daily Drops, Weekly Wave, …). Their
        // `tracks` come as id-only stubs, so hydrate them via /tracks?ids=.
        ("GET", ["system-playlists", urn]) => {
            let enc = urlencoding::encode(urn);
            match s
                .sc_get(&format!("/system-playlists/{enc}"), token.as_deref())
                .await
            {
                Ok((status, mut v)) if (200..300).contains(&status) => {
                    if let Some(tracks) = v.get_mut("tracks").and_then(Value::as_array_mut) {
                        let ids: Vec<String> = tracks
                            .iter()
                            .filter_map(|t| {
                                t.get("id")
                                    .map(|i| i.to_string().trim_matches('"').to_string())
                            })
                            .collect();
                        if !ids.is_empty() {
                            let mut by_id: HashMap<String, Value> = HashMap::new();
                            for chunk in ids.chunks(50) {
                                let path = format!("/tracks?ids={}", chunk.join(","));
                                if let Ok((st, hv)) = s.sc_get(&path, token.as_deref()).await {
                                    if (200..300).contains(&st) {
                                        if let Some(arr) = hv.as_array() {
                                            for t in arr {
                                                if let Some(id) = t
                                                    .get("id")
                                                    .map(|i| {
                                                        i.to_string().trim_matches('"').to_string()
                                                    })
                                                {
                                                    by_id.insert(id, normalize_urn(t.clone()));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            for stub in tracks.iter_mut() {
                                if let Some(id) = stub
                                    .get("id")
                                    .map(|i| i.to_string().trim_matches('"').to_string())
                                {
                                    if let Some(full) = by_id.get(&id) {
                                        *stub = full.clone();
                                    }
                                }
                            }
                        }
                    }
                    json_resp(status, &normalize_urn(v))
                }
                Ok((status, v)) => json_resp(status, &v),
                Err(e) => err(502, &e),
            }
        }
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
            match super::webview::execute(&s.app, Some(t), &method, &url, None).await {
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

        // ── debug: show/hide the writer webview for manual interaction ──
        ("POST", ["debug", "show-writer"]) => {
            if let Some(wv) = super::webview::ensure_window(&s.app) {
                if let Some(t) = token.as_deref() {
                    super::webview::inject_session(&wv, t);
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
            if let Some(wv) = s.app.get_webview_window(super::webview::LABEL) {
                let _ = wv.hide();
            }
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
