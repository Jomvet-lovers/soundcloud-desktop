use std::collections::HashMap;

use bytes::Bytes;
use serde_json::{json, Value};
use warp::http::{HeaderMap, StatusCode};
use warp::reply::{Reply, Response};

use super::super::DirectState;
use super::normalize::*;
use super::super::sc::{id_of};

// ─── helpers ─────────────────────────────────────────────────────────────

pub(crate) fn json_resp(code: u16, v: &Value) -> Response {
    let mut resp = warp::reply::json(v).into_response();
    *resp.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::OK);
    resp
}

pub(crate) fn ok(v: Value) -> Response {
    json_resp(200, &v)
}

pub(crate) fn err(code: u16, msg: &str) -> Response {
    json_resp(code, &json!({ "error": msg }))
}

pub(crate) fn page(items: Vec<Value>, page: u64, limit: u64, has_more: bool) -> Value {
    json!({
        "collection": items,
        "page": page,
        "page_size": limit,
        "has_more": has_more,
    })
}

pub(crate) fn empty_page(page_no: u64, limit: u64) -> Value {
    page(Vec::new(), page_no, limit, false)
}

pub(crate) fn q_u64(q: &HashMap<String, String>, key: &str, default: u64) -> u64 {
    q.get(key)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

pub(crate) fn q_str(q: &HashMap<String, String>, key: &str) -> Option<String> {
    q.get(key).filter(|v| !v.is_empty()).cloned()
}

pub(crate) fn parse_query(raw: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(raw.as_bytes())
        .into_owned()
        .collect()
}

pub(crate) fn token_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|t| !t.is_empty() && t != "undefined" && t != "null")
}

pub(crate) fn body_json(body: &Bytes) -> Value {
    if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(body).unwrap_or(Value::Null)
    }
}

pub(crate) fn sc_items(v: &Value) -> Vec<Value> {
    v.get("collection")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn sc_has_more(v: &Value) -> bool {
    v.get("next_href")
        .map(|n| n.is_string() && !n.is_null())
        .unwrap_or(false)
}

pub(crate) fn sc_user_items(v: &Value) -> Vec<Value> {
    sc_items(v)
}

pub(crate) async fn need_my_id(state: &DirectState, token: Option<&str>) -> Result<u64, Response> {
    let Some(t) = token else {
        return Err(err(401, "unauthorized"));
    };
    state
        .my_user_id(t)
        .await
        .map_err(|e| json_resp(502, &json!({ "error": e })))
}

pub(crate) async fn liked_track_ids(state: &DirectState, token: &str) -> Vec<String> {
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

/// Resolve title-less `{id}` stubs in place via `/tracks?ids=`. Returns the
/// number of entries that could not be resolved and were dropped, so write
/// paths can fail closed instead of persisting a truncated list.
pub(crate) async fn hydrate_track_stubs(
    state: &DirectState,
    token: Option<&str>,
    tracks: &mut Vec<Value>,
) -> usize {
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
        return 0;
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
    let before = tracks.len();
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
    let dropped = before.saturating_sub(tracks.len());
    if dropped > 0 {
        eprintln!("[direct] hydrate_track_stubs dropped {dropped}/{before} unresolved stubs");
    }
    dropped
}

pub(crate) async fn merge_local_likes(state: &DirectState, mut items: Vec<Value>, page_no: u64) -> Vec<Value> {
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

pub(crate) async fn merge_local_playlist_likes(
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
