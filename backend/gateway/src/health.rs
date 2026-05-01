use std::path::Path;
use std::sync::atomic::Ordering;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http1;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tokio::task::JoinHandle;
use tracing::info;

use crate::config::Config;
use crate::lb::{BackendPool, STATE_DOWN, STATE_UP};

pub fn spawn(cfg: Config, pool: BackendPool) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            for backend in pool.backends.iter() {
                let ok = check(&backend.socket_path, &cfg.health_path, cfg.health_timeout).await;
                let new = if ok { STATE_UP } else { STATE_DOWN };
                let prev = backend.state.swap(new, Ordering::AcqRel);
                if prev != new {
                    info!(
                        "[health] backend {} {} → {}",
                        backend.id,
                        if prev == STATE_UP { "UP" } else { "DOWN" },
                        if new == STATE_UP { "UP" } else { "DOWN" },
                    );
                }
            }
            tokio::time::sleep(cfg.health_interval).await;
        }
    })
}

async fn check(sock: &Path, path: &str, timeout: std::time::Duration) -> bool {
    let stream = match tokio::time::timeout(timeout, UnixStream::connect(sock)).await {
        Ok(Ok(s)) => s,
        _ => return false,
    };
    let io = TokioIo::new(stream);
    let (mut sender, conn) = match tokio::time::timeout(timeout, http1::handshake(io)).await {
        Ok(Ok(p)) => p,
        _ => return false,
    };
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = match Request::builder()
        .method("GET")
        .uri(path)
        .header("host", "health.local")
        .body(empty())
    {
        Ok(r) => r,
        Err(_) => return false,
    };
    match tokio::time::timeout(timeout, sender.send_request(req)).await {
        Ok(Ok(res)) => res.status().is_success(),
        _ => false,
    }
}

fn empty() -> http_body_util::combinators::BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}
