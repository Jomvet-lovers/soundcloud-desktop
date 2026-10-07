use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::sc::{SC_API, id_of, fetch_me};
use super::super::webview::{spawn_write};

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
