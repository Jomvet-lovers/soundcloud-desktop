use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::sc::{SC_API, id_of, fetch_me};
use super::super::webview::{spawn_write};
use super::super::DirectState;

pub async fn route(ctx: &Ctx) -> Result<Response, warp::Rejection> {
    let s = &ctx.state;
    let q = &ctx.q;
    let token = &ctx.token;
    let body = &ctx.body;
    let p: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (ctx.method.as_str(), p.as_slice()) {
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

        // ── feed (upstream-style activity) ────────────────────────────
        // SoundCloud's own `/stream`: what the people you follow uploaded and
        // reposted, newest first. Unlike `/me/followings/tracks` above this
        // keeps playlists too, and preserves the repost flag so the frontend
        // can show who pushed it.
        ("GET", ["me", "feed"]) => {
            let limit = q_u64(&q, "limit", 30);
            let page_no = q_u64(&q, "page", 0);
            let offset = page_no * limit;
            match token.as_deref() {
                Some(t) => match s
                    .sc_get(&format!("/stream?limit={limit}&offset={offset}"), Some(t))
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => {
                        let mut items = stream_feed_items(&v);
                        mark_liked_feed_tracks(s, t, &mut items).await;
                        ok(page(items, page_no, limit, sc_has_more(&v)))
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
                let mut store = s.store.lock().await;
                let sc_items: Vec<Value> = sc_items(&v).into_iter().map(normalize_urn).collect();
                // SoundCloud owns the privacy of its own sets: mirror it onto
                // the local copies before they're served, so the list can't
                // keep a stale badge (another client may have toggled it).
                let mut changed = false;
                for p in &sc_items {
                    let (Some(u), Some(sharing)) = (urn_of(p), normalize_sharing(p.get("sharing")))
                    else {
                        continue;
                    };
                    if !u.starts_with("soundcloud:playlists:") {
                        continue;
                    }
                    changed |= store.set_playlist_sharing(&u, sharing);
                }
                if changed {
                    store.save();
                    if page_no == 0 {
                        // Re-clone so the fresh sharing reaches this response.
                        items.clear();
                        items.extend(store.playlists.iter().cloned());
                    }
                }
                let sc_items: Vec<Value> = sc_items
                    .into_iter()
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

        _ => err(404, "not found"),
    };
    Ok(resp)
}

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

/// `/stream` item -> the frontend `FeedItem` shape (`FeedItem` in hooks.ts).
///
/// Two envelope shapes arrive from SoundCloud:
/// - a plain upload inlines the object: `{type: "track"|"playlist", track|playlist: {…}}`
/// - a repost nests it one level down: `{type: "track-repost"|"playlist-repost", origin: {kind, …}}`
///
/// The SC `type` is passed through untouched (the `-repost` suffix is what the
/// UI reads to draw the "X reposted" line), while the renderable kind is taken
/// from the payload itself. Anything that is neither a track nor a playlist is
/// dropped — the feed only has rows for those two.
fn stream_feed_items(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for it in sc_items(v) {
        let Some(sc_type) = it.get("type").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        // `origin` wins over the inlined key: a repost only has `origin`, and
        // when both exist `origin` is the fuller copy.
        let Some(body) = it
            .get("origin")
            .or_else(|| it.get("track"))
            .or_else(|| it.get("playlist"))
            .filter(|b| !b.is_null())
            .cloned()
        else {
            continue;
        };
        // `kind` is authoritative; the `type` prefix is the fallback (older
        // repost payloads carry no `kind` on `origin`).
        let kind = body
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                sc_type
                    .split('-')
                    .next()
                    .unwrap_or("track")
                    .to_string()
            });
        let render = if kind == "album" { "playlist" } else { kind.as_str() };
        if render != "track" && render != "playlist" {
            continue;
        }
        let id = body
            .get("id")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_default();
        let created_at = it
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        // Item-level `user` is the reposter on `*-repost` rows (and the
        // uploader on plain ones) — the "X reposted" line reads it.
        let user = it
            .get("user")
            .filter(|u| u.is_object())
            .cloned()
            .unwrap_or(Value::Null);
        // `uuid` numbers repeat across pages, so the key is the object id plus
        // the feed timestamp — a track uploaded and reposted at the same
        // instant would still differ by the SC `type` prefix.
        out.push(json!({
            "id": format!("{sc_type}:{id}:{created_at}"),
            "type": sc_type,
            "kind": render,
            "created_at": created_at,
            "user": user,
            "origin": normalize_urn(body),
        }));
    }
    out
}

/// `/stream` sends no `user_favorite`, so the like buttons on the feed would
/// read wrong until the user's liked-tracks list happened to be cached
/// elsewhere. Stamp the flag from `/me/track_likes/ids` (one extra call per
/// page) — `LikeButton` seeds its global store from it.
async fn mark_liked_feed_tracks(state: &DirectState, token: &str, items: &mut [Value]) {
    let liked = liked_track_ids(state, token).await;
    if liked.is_empty() {
        return;
    }
    let ids: std::collections::HashSet<&str> = liked.iter().map(String::as_str).collect();
    for item in items.iter_mut() {
        let Some(origin) = item.get_mut("origin") else {
            continue;
        };
        if origin.get("kind").and_then(Value::as_str) != Some("track") {
            continue;
        }
        let liked = origin
            .get("id")
            .and_then(Value::as_u64)
            .map(|n| ids.contains(n.to_string().as_str()))
            .unwrap_or(false);
        if let Some(obj) = origin.as_object_mut() {
            obj.insert("user_favorite".into(), json!(liked));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{stream_feed_items, stream_to_tracks};
    use serde_json::json;

    fn track(id: u64, title: &str) -> serde_json::Value {
        json!({ "id": id, "kind": "track", "title": title, "duration": 1000 })
    }

    fn playlist(id: u64, title: &str) -> serde_json::Value {
        json!({
            "id": id, "kind": "playlist", "title": title, "track_count": 3,
            "artwork_url": null, "user": null, "tracks": [],
        })
    }

    /// A plain upload inlines its payload under `track` / `playlist`.
    #[test]
    fn stream_feed_maps_inlined_track_and_playlist() {
        let out = stream_feed_items(&json!({ "collection": [
            { "type": "track", "created_at": "2026-09-20T03:00:41Z", "uuid": "1",
              "user": { "id": 1547398104, "kind": "user", "username": "nkdhr" },
              "track": track(11, "a") },
            { "type": "playlist", "created_at": "2026-09-12T20:01:35Z", "uuid": "2",
              "user": { "id": 1, "kind": "user", "username": "natsumi" },
              "playlist": playlist(22, "p") },
        ] }));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].get("type").and_then(|v| v.as_str()), Some("track"));
        assert_eq!(
            out[0].get("origin").and_then(|o| o.get("urn")).and_then(|u| u.as_str()),
            Some("soundcloud:tracks:11"),
        );
        assert_eq!(out[1].get("type").and_then(|v| v.as_str()), Some("playlist"));
        assert_eq!(
            out[1].get("origin").and_then(|o| o.get("urn")).and_then(|u| u.as_str()),
            Some("soundcloud:playlists:22"),
        );
        assert_eq!(out[0].get("created_at").and_then(|v| v.as_str()), Some("2026-09-20T03:00:41Z"));
        // The item-level `user` (reposter / uploader) is passed through.
        assert_eq!(
            out[0].get("user").and_then(|u| u.get("username")).and_then(|v| v.as_str()),
            Some("nkdhr"),
        );
    }

    /// A repost nests the payload under `origin` — the SC `type` (with its
    /// `-repost` suffix) must survive so the UI can label it.
    #[test]
    fn stream_feed_keeps_repost_type_and_origin() {
        let out = stream_feed_items(&json!({ "collection": [
            { "type": "track-repost", "created_at": "2026-10-01T00:00:00Z", "uuid": "1",
              "origin": track(33, "b") },
        ] }));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].get("type").and_then(|v| v.as_str()), Some("track-repost"));
        assert_eq!(
            out[0].get("origin").and_then(|o| o.get("id")).and_then(|v| v.as_u64()),
            Some(33),
        );
    }

    /// Entries without any payload (SC's empty `origin` rows) are skipped.
    #[test]
    fn stream_feed_skips_payloadless_entries() {
        let out = stream_feed_items(&json!({ "collection": [
            { "type": "track", "created_at": "2026-10-01T00:00:00Z", "uuid": "1", "track": null },
            { "type": "track", "created_at": "2026-10-01T00:00:00Z", "uuid": "2" },
        ] }));
        assert!(out.is_empty());
    }

    /// Albums render as playlists (the frontend has one row shape for sets).
    #[test]
    fn stream_feed_maps_album_kind_to_playlist_row() {
        let mut album = playlist(44, "al");
        album["kind"] = json!("album");
        let out = stream_feed_items(&json!({ "collection": [
            { "type": "playlist", "created_at": "2026-10-01T00:00:00Z", "uuid": "1",
              "playlist": album },
        ] }));
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].get("origin").and_then(|o| o.get("urn")).and_then(|u| u.as_str()),
            Some("soundcloud:playlists:44"),
        );
    }

    /// Repost keys must not collide with the original upload's key, nor across
    /// pages that reuse the same `uuid` numbering.
    #[test]
    fn stream_feed_keys_are_unique() {
        let out = stream_feed_items(&json!({ "collection": [
            { "type": "track", "created_at": "2026-10-01T00:00:00Z", "uuid": "1", "track": track(5, "x") },
            { "type": "track-repost", "created_at": "2026-10-01T00:00:00Z", "uuid": "2",
              "origin": track(5, "x") },
        ] }));
        assert_ne!(out[0].get("id"), out[1].get("id"));
    }

    /// The track-only arm used by `/me/followings/tracks` still dedupes by URN
    /// and ignores playlists.
    #[test]
    fn stream_to_tracks_dedupes_and_drops_playlists() {
        let out = stream_to_tracks(&json!({ "collection": [
            { "type": "track", "track": track(1, "a") },
            { "type": "track", "track": track(1, "a") },
            { "type": "playlist", "playlist": playlist(2, "p") },
        ] }));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].get("urn").and_then(|u| u.as_str()), Some("soundcloud:tracks:1"));
    }
}

