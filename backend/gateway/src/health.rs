use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http1;
use hyper::header::{CONNECTION, HOST};
use hyper::{HeaderMap, Request};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tokio::task::JoinHandle;
use tracing::info;

use crate::config::Config;
use crate::lb::{BackendPool, STATE_DOWN, STATE_UP};

const CONN_DEADLINE: Duration = Duration::from_secs(10);

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

async fn check(sock: &Path, path: &str, timeout: Duration) -> bool {
    let stream = match tokio::time::timeout(timeout, UnixStream::connect(sock)).await {
        Ok(Ok(s)) => s,
        _ => return false,
    };
    let io = TokioIo::new(stream);
    let (mut sender, conn) = match tokio::time::timeout(timeout, http1::handshake(io)).await {
        Ok(Ok(p)) => p,
        _ => return false,
    };
    // Drive connection in a child task with a hard deadline so it can never leak.
    let conn_handle = tokio::spawn(async move {
        let _ = tokio::time::timeout(CONN_DEADLINE, conn).await;
    });

    let mut headers = HeaderMap::new();
    headers.insert(HOST, "health.local".parse().unwrap());
    headers.insert(CONNECTION, "close".parse().unwrap());

    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(h) = builder.headers_mut() {
        *h = headers;
    }
    let req = match builder.body(empty()) {
        Ok(r) => r,
        Err(_) => {
            conn_handle.abort();
            return false;
        }
    };

    let ok = match tokio::time::timeout(timeout, sender.send_request(req)).await {
        Ok(Ok(res)) => {
            let success = res.status().is_success();
            // Drain the body so the upstream can close cleanly.
            let _ = res.into_body().collect().await;
            success
        }
        _ => false,
    };
    drop(sender);
    // Connection: close → server FINs after response → conn task exits on its own.
    // Hard cap via spawn timeout above; abort here belt-and-suspenders.
    conn_handle.abort();
    ok
}

fn empty() -> http_body_util::combinators::BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}
