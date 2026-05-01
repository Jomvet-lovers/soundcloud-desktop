use std::sync::atomic::Ordering;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{Response, StatusCode};
use serde_json::json;

use crate::lb::{BackendPool, STATE_UP};

type Body = BoxBody<Bytes, hyper::Error>;

pub async fn handle(pool: &BackendPool, start: Instant) -> Response<Body> {
    let up = pool
        .backends
        .iter()
        .filter(|b| b.state.load(Ordering::Acquire) == STATE_UP)
        .count();

    let status = if up == 0 { "down" } else { "ok" };
    let body = json!({
        "status": status,
        "uptime_secs": start.elapsed().as_secs(),
    });

    let code = if up == 0 {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let payload = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(code)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::from(payload)).map_err(|n| match n {}).boxed())
        .unwrap()
}
