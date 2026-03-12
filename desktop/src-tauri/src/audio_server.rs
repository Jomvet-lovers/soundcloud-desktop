use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;
use warp::Filter;

use crate::server::cors;

fn content_type_for(filename: &str) -> &'static str {
    if filename.ends_with(".png") {
        "image/png"
    } else if filename.ends_with(".webp") {
        "image/webp"
    } else if filename.ends_with(".gif") {
        "image/gif"
    } else if filename.ends_with(".svg") {
        "image/svg+xml"
    } else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}

async fn serve_file(
    filename: String,
    dir: PathBuf,
    content_type: &str,
    range_header: Option<String>,
) -> Result<Response<Body>, warp::Rejection> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap());
    }

    let path = dir.join(&filename);
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
                .header("Content-Type", content_type)
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
        .header("Content-Type", content_type)
        .header("Content-Length", total.to_string())
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from(buf))
        .unwrap())
}

pub async fn start(audio_dir: PathBuf, wallpapers_dir: PathBuf) -> u16 {
    let audio = audio_dir.clone();
    let wallpapers = wallpapers_dir.clone();

    let audio_route = warp::path("audio")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::header::optional::<String>("range"))
        .and_then(move |filename: String, range: Option<String>| {
            let dir = audio.clone();
            async move { serve_file(filename, dir, "audio/mpeg", range).await }
        });

    let wallpaper_route = warp::path("wallpapers")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and_then(move |filename: String| {
            let dir = wallpapers.clone();
            async move {
                let ct = content_type_for(&filename);
                serve_file(filename, dir, ct, None).await
            }
        });

    let routes = audio_route.or(wallpaper_route).with(cors());

    let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
    let (addr, server) = warp::serve(routes).bind_ephemeral(addr);
    tokio::spawn(server);

    println!("[AudioServer] http://127.0.0.1:{}", addr.port());
    addr.port()
}
