use std::collections::HashMap;

use serde_json::{json, Value};
use warp::reply::Response;

use super::Ctx;
use super::common::*;
use super::normalize::*;
use super::super::sc::{id_of};
use std::time::{Duration, Instant};

pub async fn route(ctx: &Ctx) -> Result<Response, warp::Rejection> {
    let s = &ctx.state;
    let q = &ctx.q;
    let token = &ctx.token;
    let p: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (ctx.method.as_str(), p.as_slice()) {
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
        // sc.com/you/history is the source of truth when signed in: same
        // api-v2 endpoint the web app uses, cursor paging via the
        // `from`/`offset` pair carried in `next_href`. The local store
        // remains the offline fallback.

        _ => err(404, "not found"),
    };
    Ok(resp)
}
