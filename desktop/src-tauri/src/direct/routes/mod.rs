//! Local HTTP API implementing the subset of the app backend that the
//! frontend uses. Reads are resolved against SoundCloud api-v2; writes are
//! persisted locally in [`super::store::LocalStore`] and best-effort synced
//! through the hidden writer webview.
//!
//! Route handlers live in one module per domain (`me`, `tracks`, `playlists`,
//! `catalog`, `discover`, `local`); shared plumbing is in `common` and value
//! mapping in `normalize`. This file only decodes the request, dispatches on
//! the first path segment and serves the tiny meta/auth arms.

pub mod catalog;
pub mod common;
pub mod discover;
pub mod local;
pub mod me;
pub mod normalize;
pub mod playlists;
pub mod tracks;

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use serde_json::json;
use warp::http::{HeaderMap, Method};
use warp::reply::Response;
use warp::{path::FullPath, Filter};

use super::DirectState;

pub struct Ctx {
    pub state: Arc<DirectState>,
    pub method: String,
    pub segs: Vec<String>,
    pub q: HashMap<String, String>,
    pub body: Bytes,
    pub token: Option<String>,
}

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
    let q = common::parse_query(&raw_query);
    let token = common::token_of(&headers);
    let m = method.as_str();
    let ctx = Ctx {
        state,
        method: m.to_string(),
        segs: decoded,
        q,
        body,
        token,
    };
    let segs: Vec<&str> = ctx.segs.iter().map(String::as_str).collect();
    let resp = match (m, segs.as_slice()) {
        // ── meta ────────────────────────────────────────────────────
        ("GET", ["health"]) => common::ok(json!({"status": "ok"})),

        // ── auth ────────────────────────────────────────────────────
        ("GET", ["auth", "status"]) => common::ok(json!({
            "authenticated": ctx.token.is_some(),
            "tokenState": "ok",
            "pendingSyncCount": 0,
            "failedSyncCount": 0,
        })),
        ("POST", ["auth", "refresh"]) => common::ok(json!({})),
        ("POST", ["auth", "logout"]) => common::ok(json!({})),
        ("POST", ["auth", "link", "create"]) | ("POST", ["auth", "link", "claim"]) => {
            common::err(501, "direct mode: not supported")
        }

        // ── streaming/storage fallbacks (anon path handles audio) ───
        ("GET", ["stream", ..]) | ("GET", ["download", ..]) | ("GET", ["storage", ..])
        | ("GET", ["redirect", ..]) => common::err(404, "direct mode: use anon path"),

        (_, [first, ..]) => match *first {
            "me" | "likes" => me::route(&ctx).await?,
            "tracks" | "tags" | "comments" => tracks::route(&ctx).await?,
            "playlists" => playlists::route(&ctx).await?,
            "users" | "albums" | "artists" => catalog::route(&ctx).await?,
            "search" | "featured" | "discover" | "system-playlists" | "recommendations" => {
                discover::route(&ctx).await?
            }
            "history" | "events" | "indexing" | "dislikes" | "debug" => local::route(&ctx).await?,
            _ => common::err(404, "not found"),
        },
        _ => common::err(404, "not found"),
    };

    Ok(resp)
}
