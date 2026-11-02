use std::collections::HashMap;

use bytes::Bytes;
use serde_json::{json, Value};
use warp::http::{HeaderMap, StatusCode};
use warp::reply::{Reply, Response};

use super::super::DirectState;
use super::normalize::*;
use super::super::sc::{id_of, SC_API};

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

/// Send a write through the writer WebView and require SoundCloud to accept it.
///
/// Callers touch the local store only after this returns `Ok`, so a rejected or
/// unreachable write can never leave the UI showing a change SoundCloud does not
/// have. `ok_404` treats "already gone" as success (removing a like or follow
/// that SoundCloud already dropped).
pub(crate) async fn mirror_write(
    s: &DirectState,
    token: &str,
    method: &str,
    url: &str,
    body: Option<&Value>,
    ok_404: bool,
) -> Result<(), String> {
    match super::super::webview::execute(&s.app, Some(token), method, url, body).await {
        Ok(r) if (200..300).contains(&r.status) => Ok(()),
        Ok(r) if ok_404 && r.status == 404 => Ok(()),
        Ok(r) => {
            eprintln!(
                "[sync] {method} {url} -> {} captcha={} body={}",
                r.status,
                r.captcha.is_some(),
                r.payload.chars().take(200).collect::<String>()
            );
            Err("SoundCloud rejected the change; nothing was changed".into())
        }
        Err(e) => {
            eprintln!("[sync] {method} {url} failed: {e}");
            Err("SoundCloud unreachable; nothing was changed".into())
        }
    }
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

/// Verbatim `next_href` of a page — the opaque cursor `/me/likes/tracks` hands
/// to the frontend and takes back as `?cursor=`.
pub(crate) fn sc_next_cursor(v: &Value) -> Option<String> {
    v.get("next_href")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Path for one page of a cursor-paged user collection (`track_likes`,
/// `playlist_likes`).
///
/// Both are cursor-paged (`ChronoParams.Cursor`): the real cursor is the opaque
/// `offset` value inside the previous page's `next_href`, so a page is addressed
/// by replaying that URL. Neither survives numeric paging — `track_likes`
/// rejects it (400 `invalid cursor format`) and `playlist_likes` silently serves
/// page 1 again — so the cursor is the only way through. Only our own api-v2
/// host is followed (`sc_get` reattaches it with a fresh `client_id`), which also
/// keeps a crafted cursor from steering the read at another host.
pub(crate) fn user_cursor_path(
    collection: &str,
    user_id: impl std::fmt::Display,
    limit: u64,
    cursor: Option<&str>,
) -> String {
    // A bare prefix match is not a host check: `https://api-v2.soundcloud.com@evil`
    // and `https://api-v2.soundcloud.com.evil` both "start with" SC_API, and the
    // former sends the request — with the caller's OAuth header — to `evil`.
    // Demand a path separator right after the origin.
    match cursor.and_then(|c| c.strip_prefix(SC_API)) {
        Some(rest) if rest.starts_with('/') || rest.starts_with('?') => rest.to_string(),
        _ => format!("/users/{user_id}/{collection}?limit={limit}&offset=0"),
    }
}

/// The object behind each row of a likes page: rows wrap it as `track` or
/// `playlist`. Rows without one are dropped, as SoundCloud leaves holes for
/// deleted items.
pub(crate) fn wrapped_items(v: &Value, key: &str) -> Vec<Value> {
    sc_items(v)
        .into_iter()
        .filter_map(|it| it.get(key).cloned())
        .filter(|t| !t.is_null())
        .map(normalize_urn)
        .collect()
}

/// Page envelope carrying the next cursor when SoundCloud offered one.
///
/// `has_more` follows the cursor, not the row count: SoundCloud returns short
/// pages mid-collection (29 of a 30 limit) and still has more, and it also keeps
/// reporting a cursor on the last page — which the caller detects by following it
/// to an empty page.
pub(crate) fn cursor_page(
    items: Vec<Value>,
    page_no: u64,
    limit: u64,
    next_cursor: Option<String>,
) -> Value {
    let mut body = page(items, page_no, limit, next_cursor.is_some());
    if let Some(c) = next_cursor {
        body["next_cursor"] = json!(c);
    }
    body
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

pub(crate) async fn merge_local_likes(
    s: &DirectState,
    owner: Option<u64>,
    mut items: Vec<Value>,
    page_no: u64,
) -> Vec<Value> {
    let store = s.store_for(owner).await;
    let store = store.lock().await;
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
    s: &DirectState,
    owner: Option<u64>,
    mut items: Vec<Value>,
    page_no: u64,
) -> Vec<Value> {
    let store = s.store_for(owner).await;
    let store = store.lock().await;
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

/// Privacy values the sharing PUTs accept. Anything else is a client bug and
/// must not reach SoundCloud, where a bad value could flip a set public.
pub(crate) fn normalize_sharing(v: Option<&Value>) -> Option<&'static str> {
    match v.and_then(Value::as_str) {
        Some("public") => Some("public"),
        Some("private") => Some("private"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{cursor_page, normalize_sharing, user_cursor_path, wrapped_items, SC_API};
    use serde_json::json;

    #[test]
    fn sharing_only_accepts_known_values() {
        assert_eq!(normalize_sharing(Some(&json!("public"))), Some("public"));
        assert_eq!(normalize_sharing(Some(&json!("private"))), Some("private"));
        assert_eq!(normalize_sharing(Some(&json!("unlisted"))), None);
        assert_eq!(normalize_sharing(Some(&json!(7))), None);
        assert_eq!(normalize_sharing(None), None);
    }

    #[test]
    fn cursor_collections_are_addressed_by_cursor() {
        for collection in ["track_likes", "playlist_likes"] {
            let first = format!("/users/42/{collection}?limit=30&offset=0");
            assert_eq!(user_cursor_path(collection, 42, 30, None), first);
            // The cursor is the whole api-v2 URL: the host is reattached by
            // sc_get.
            let cursor =
                format!("{SC_API}/users/42/{collection}?limit=30&offset=2026-04-16T23%3A31%3A23Z");
            assert_eq!(
                user_cursor_path(collection, 42, 30, Some(&cursor)),
                format!("/users/42/{collection}?limit=30&offset=2026-04-16T23%3A31%3A23Z")
            );
            // Anything not on our own api-v2 host falls back to the first page
            // instead of being replayed against someone else's server.
            assert_eq!(
                user_cursor_path(collection, 42, 30, Some("https://evil.example/x")),
                first
            );
            // Host confusion: a bare prefix match would accept both of these and
            // send the OAuth header to the attacker's host.
            for sneaky in [
                format!("{SC_API}@evil.example/x"),
                format!("{SC_API}.evil.example/x"),
            ] {
                assert_eq!(user_cursor_path(collection, 42, 30, Some(&sneaky)), first);
            }
            assert_eq!(user_cursor_path(collection, 42, 30, Some("")), first);
        }
    }

    #[test]
    fn likes_rows_keep_only_the_wrapped_object() {
        let v = json!({
            "collection": [
                { "track": { "id": 1, "kind": "track", "urn": "soundcloud:tracks:1" } },
                { "track": null },
                {},
                { "track": { "id": 2, "kind": "track", "urn": "soundcloud:tracks:2" } }
            ]
        });
        let items = wrapped_items(&v, "track");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["urn"], json!("soundcloud:tracks:1"));
        assert_eq!(items[1]["urn"], json!("soundcloud:tracks:2"));

        // A page of playlist likes wraps `playlist` rows and ignores the rest.
        let v = json!({
            "collection": [
                { "playlist": { "id": 7, "kind": "playlist", "urn": "soundcloud:playlists:7" } },
                { "track": { "id": 9, "kind": "track", "urn": "soundcloud:tracks:9" } }
            ]
        });
        let items = wrapped_items(&v, "playlist");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["urn"], json!("soundcloud:playlists:7"));
    }

    #[test]
    fn cursor_page_reports_more_only_with_a_cursor() {
        let body = cursor_page(vec![json!({ "id": 1 })], 0, 30, Some("next".into()));
        assert_eq!(body["has_more"], json!(true));
        assert_eq!(body["next_cursor"], json!("next"));
        assert_eq!(body["page"], json!(0));
        assert_eq!(body["page_size"], json!(30));

        let body = cursor_page(Vec::new(), 0, 30, None);
        assert_eq!(body["has_more"], json!(false));
        assert!(body.get("next_cursor").is_none());
    }
}
