use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::TryStreamExt;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;

use crate::constants::{PROXY_URL, is_domain_whitelisted};

pub async fn handle_proxy(
    encoded_url: String,
    method: warp::http::Method,
    headers: warp::http::HeaderMap,
    body: warp::hyper::body::Bytes,
    http_client: reqwest::Client,
) -> Result<Response<Body>, warp::Rejection> {
    let target_url = match BASE64.decode(encoded_url.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("invalid utf8"))
                    .unwrap());
            }
        },
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("invalid base64"))
                .unwrap());
        }
    };

    let host = target_url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.split(':').next())
        .unwrap_or("");

    if is_domain_whitelisted(host) {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("whitelisted domain"))
            .unwrap());
    }

    let encoded_for_header = BASE64.encode(target_url.as_bytes());
    #[cfg(debug_assertions)]
    println!("[Proxy] {} {} -> X-Target", method, target_url);

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut req = http_client
        .request(reqwest_method, PROXY_URL)
        .header("X-Target", &encoded_for_header);

    for (key, value) in headers.iter() {
        let name = key.as_str();
        if matches!(name, "content-type" | "range" | "accept" | "accept-encoding" | "authorization") {
            req = req.header(name, value.as_bytes());
        }
    }

    if !body.is_empty() {
        req = req.body(body.to_vec());
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("[Proxy] upstream error: {e}");
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from(format!("upstream error: {e}")))
                .unwrap());
        }
    };

    let status = upstream.status().as_u16();
    let mut builder = Response::builder().status(status);

    for (key, value) in upstream.headers().iter() {
        let name = key.as_str();
        if matches!(name, "content-type" | "content-length" | "cache-control" | "etag" | "last-modified" | "accept-ranges" | "content-range") {
            builder = builder.header(name, value.as_bytes());
        }
    }

    let stream = upstream
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let body = Body::wrap_stream(stream);

    Ok(builder.body(body).unwrap())
}
