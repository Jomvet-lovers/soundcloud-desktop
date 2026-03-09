use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;
use warp::Filter;

use crate::proxy::handle_proxy;

pub struct CacheServerState {
    pub port: u16,
}

#[tauri::command]
pub fn get_cache_server_port(state: tauri::State<'_, std::sync::Arc<CacheServerState>>) -> u16 {
    state.port
}

async fn serve_audio(
    filename: String,
    cache_dir: PathBuf,
    range_header: Option<String>,
) -> Result<Response<Body>, warp::Rejection> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap());
    }

    let path = cache_dir.join(&filename);
    let mut file = match File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap());
        }
    };

    let metadata = file.metadata().await.unwrap();
    let total = metadata.len();

    if let Some(range) = range_header {
        if let Some(range_val) = range.strip_prefix("bytes=") {
            let parts: Vec<&str> = range_val.splitn(2, '-').collect();
            let start: u64 = parts[0].parse().unwrap_or(0);
            let end: u64 = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse().unwrap_or(total - 1)
            } else {
                total - 1
            };

            if start >= total {
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("Content-Range", format!("bytes */{total}"))
                    .body(Body::empty())
                    .unwrap());
            }

            let length = end - start + 1;
            let mut buf = vec![0u8; length as usize];
            tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(start))
                .await
                .unwrap();
            file.read_exact(&mut buf).await.unwrap_or_default();

            return Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("Content-Type", "audio/mpeg")
                .header("Content-Length", length.to_string())
                .header("Content-Range", format!("bytes {start}-{end}/{total}"))
                .header("Accept-Ranges", "bytes")
                .header("Access-Control-Allow-Origin", "*")
                .body(Body::from(buf))
                .unwrap());
        }
    }

    let mut buf = Vec::with_capacity(total as usize);
    file.read_to_end(&mut buf).await.unwrap_or_default();

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "audio/mpeg")
        .header("Content-Length", total.to_string())
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from(buf))
        .unwrap())
}

pub async fn start_cache_server(cache_dir: PathBuf) -> u16 {
    let dir = cache_dir.clone();

    let audio_route = warp::path("audio")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::header::optional::<String>("range"))
        .and_then(move |filename: String, range: Option<String>| {
            let dir = dir.clone();
            async move { serve_audio(filename, dir, range).await }
        });

    let http_client = reqwest::Client::new();
    let proxy_route = warp::path("p")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::method())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and({
            let c = http_client.clone();
            warp::any().map(move || c.clone())
        })
        .and_then(
            |encoded_url: String,
             method: warp::http::Method,
             headers: warp::http::HeaderMap,
             body: warp::hyper::body::Bytes,
             client: reqwest::Client| {
                handle_proxy(encoded_url, method, headers, body, client)
            },
        );

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec![
            "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS",
        ])
        .allow_headers(vec![
            "range",
            "content-type",
            "accept",
            "authorization",
            "accept-encoding",
        ])
        .expose_headers(vec!["content-range", "content-length", "accept-ranges"]);

    let routes = audio_route.or(proxy_route).with(cors);

    let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
    let (addr, server) = warp::serve(routes).bind_ephemeral(addr);

    tokio::spawn(server);

    println!(
        "[CacheServer] Listening on http://127.0.0.1:{}",
        addr.port()
    );
    addr.port()
}
