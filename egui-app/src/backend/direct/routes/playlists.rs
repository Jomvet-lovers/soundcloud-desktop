use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::DirectState;
use super::super::sc::{SC_API, id_of, fetch_me};
use crate::backend::writer::{spawn_write, emit_sync_error};

pub async fn route(ctx: &Ctx) -> Result<Response, warp::Rejection> {
    let s = &ctx.state;
    let q = &ctx.q;
    let token = &ctx.token;
    let body = &ctx.body;
    let p: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (ctx.method.as_str(), p.as_slice()) {
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
                    match crate::backend::writer::execute_with(
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
            // System mixes (Trending by genre, Daily Drops, …) live under their
            // own endpoint — the playlist page opens them like any other set.
            if urn.starts_with("soundcloud:system-playlists:") {
                let enc = urlencoding::encode(urn);
                return match s
                    .sc_get(&format!("/system-playlists/{enc}"), token.as_deref())
                    .await
                {
                    Ok((status, mut v)) if (200..300).contains(&status) => {
                        if let Some(slot) = v.get_mut("tracks").and_then(Value::as_array_mut) {
                            let mut list = std::mem::take(slot);
                            hydrate_track_stubs(s, token.as_deref(), &mut list).await;
                            *slot = list;
                        }
                        // `normalize_deep` mirrors calculated_artwork_url into
                        // artwork_url — system playlists only carry the former.
                        Ok(json_resp(status, &normalize_deep(v)))
                    }
                    Ok((status, v)) => Ok(json_resp(status, &v)),
                    Err(e) => Ok(err(502, &e)),
                };
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
            // System mixes: their tracks ride the system-playlist payload.
            if urn.starts_with("soundcloud:system-playlists:") {
                let enc = urlencoding::encode(urn);
                let v = match s
                    .sc_get(&format!("/system-playlists/{enc}"), token.as_deref())
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => v,
                    Ok((status, v)) => return Ok(json_resp(status, &v)),
                    Err(e) => return Ok(err(502, &e)),
                };
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
                return Ok(ok(page(slice, page_no, limit, has_more)));
            }
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
            // 非 2xx はそのまま返す (空の「成功」を返さない)。
            match s.sc_get(&format!("/playlists/{id}"), token.as_deref()).await {
                Ok((status, v)) if (200..300).contains(&status) => {
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
                Ok((status, v)) => json_resp(status, &v),
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
            //
            // 土台が完全に取得できない場合は **何も書き込まない**
            // (1曲だけのローカル保存/SC 上書きというデータ損失を防ぐ)。
            let local = repaired_local_playlist(s, token.as_deref(), urn).await;
            let mut playlist = local.clone();
            let mut tracks: Vec<Value> = local
                .as_ref()
                .and_then(|lp| lp.get("tracks").and_then(Value::as_array).cloned())
                .unwrap_or_default();
            let mut base_complete = local.is_some() || !is_sc;
            if playlist.is_none() && is_sc {
                let id = id_of(urn);
                let detail = match s
                    .sc_get(&format!("/playlists/{id}"), token.as_deref())
                    .await
                {
                    Ok((status, v)) if (200..300).contains(&status) => Some(normalize_urn(v)),
                    _ => None,
                };
                let Some(detail) = detail else {
                    return Ok(err(502, "playlist base fetch failed; refusing to modify"));
                };
                if let Some(list) = detail.get("tracks").and_then(Value::as_array) {
                    let expected = list.len();
                    let mut list = list.clone();
                    let dropped = hydrate_track_stubs(s, token.as_deref(), &mut list).await;
                    if dropped > 0 || list.len() != expected {
                        return Ok(err(502, "playlist base incomplete; refusing to modify"));
                    }
                    tracks = list.into_iter().map(normalize_urn).collect();
                }
                playlist = Some(detail);
                base_complete = true;
            }
            if !base_complete {
                return Ok(err(502, "playlist base unavailable; refusing to modify"));
            }

            if let Some(track) = fetched {
                append_track_unique(&mut tracks, track);
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

            if is_sc && base_complete {
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

        _ => err(404, "not found"),
    };
    Ok(resp)
}

/// `add` を重複なく tracks に追記する (純粋関数)。追加したら true。
fn append_track_unique(tracks: &mut Vec<Value>, track: Value) -> bool {
    let t_urn = urn_of(&track).unwrap_or_default();
    if tracks
        .iter()
        .any(|t| urn_of(t).as_deref() == Some(t_urn.as_str()))
    {
        return false;
    }
    tracks.push(track);
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    fn t(urn: &str) -> Value {
        json!({ "urn": urn, "title": "x" })
    }

    #[test]
    fn append_track_unique_dedupes() {
        let mut tracks = vec![t("soundcloud:tracks:1")];
        assert!(!append_track_unique(&mut tracks, t("soundcloud:tracks:1")));
        assert_eq!(tracks.len(), 1);
        assert!(append_track_unique(&mut tracks, t("soundcloud:tracks:2")));
        assert_eq!(tracks.len(), 2);
    }
}
