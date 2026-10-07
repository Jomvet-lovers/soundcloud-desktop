use std::collections::HashMap;

use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::sc::{SC_API, id_of};

pub async fn route(ctx: &Ctx) -> Result<Response, warp::Rejection> {
    let s = &ctx.state;
    let q = &ctx.q;
    let token = &ctx.token;
    let p: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (ctx.method.as_str(), p.as_slice()) {
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

        _ => err(404, "not found"),
    };
    Ok(resp)
}
