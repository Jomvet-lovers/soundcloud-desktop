use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::client::conn::http1 as client_http1;
use hyper::header::{HeaderName, HeaderValue, CONNECTION};
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::server::{Acceptor, ServerConfig};
use tokio::net::{TcpListener, UnixStream};
use tokio::task::JoinHandle;
use tokio_rustls::LazyConfigAcceptor;
use tracing::{debug, error, warn};

use crate::acme::issuer::handle_http01;
use crate::config::Config;
use crate::lb::BackendPool;
use crate::status;
use crate::tls::{alpn_resolver, TlsState};

const ACME_TLS_ALPN: &[u8] = b"acme-tls/1";

const UDS_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const UDS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const UPSTREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
// Hard cap on backend connection lifetime (covers slow streams + safety net
// against a buggy upstream that won't close after `Connection: close`).
const UPSTREAM_CONN_DEADLINE: Duration = Duration::from_secs(300);

type ResponseBody = BoxBody<Bytes, hyper::Error>;

pub async fn serve_http(
    cfg: Config,
    pool: BackendPool,
    tls: Option<TlsState>,
    start: Instant,
) -> std::io::Result<JoinHandle<()>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.http_port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("[proxy] http listener on :{}", cfg.http_port);

    Ok(tokio::spawn(async move {
        let cfg = Arc::new(cfg);
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    error!("[proxy] http accept: {e}");
                    continue;
                }
            };
            let cfg = cfg.clone();
            let pool = pool.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let svc = service_fn(move |req| {
                    let cfg = cfg.clone();
                    let pool = pool.clone();
                    let tls = tls.clone();
                    async move { Ok::<_, hyper::Error>(handle_http(req, cfg, pool, tls, peer, start).await) }
                });
                if let Err(e) = server_http1::Builder::new()
                    .serve_connection(io, svc)
                    .with_upgrades()
                    .await
                {
                    debug!("[proxy] http conn from {peer}: {e}");
                }
            });
        }
    }))
}

async fn handle_http(
    req: Request<Incoming>,
    cfg: Arc<Config>,
    pool: BackendPool,
    tls: Option<TlsState>,
    peer: SocketAddr,
    start: Instant,
) -> Response<ResponseBody> {
    if let Some(t) = &tls {
        if req.uri().path().starts_with("/.well-known/acme-challenge/") {
            return handle_http01(req, t.http_challenges.clone()).await;
        }
    }
    if req.uri().path() == cfg.health_path {
        return status::handle(&pool, start).await;
    }
    if cfg.redirect_http && tls.is_some() {
        return redirect_to_https(&req, cfg.https_port);
    }
    proxy_request(req, pool, peer, false).await
}

fn redirect_to_https(req: &Request<Incoming>, https_port: u16) -> Response<ResponseBody> {
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty() {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(empty())
            .unwrap();
    }
    let authority = if https_port == 443 {
        host.to_string()
    } else {
        format!("{host}:{https_port}")
    };
    let path_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let location = format!("https://{authority}{path_query}");
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(hyper::header::LOCATION, location)
        .body(empty())
        .unwrap()
}

pub async fn serve_https(
    cfg: Config,
    pool: BackendPool,
    tls: TlsState,
    start: Instant,
) -> std::io::Result<JoinHandle<()>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.https_port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("[proxy] https listener on :{}", cfg.https_port);

    Ok(tokio::spawn(async move {
        let cfg = Arc::new(cfg);
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    error!("[proxy] https accept: {e}");
                    continue;
                }
            };
            let cfg = cfg.clone();
            let pool = pool.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                handle_https_conn(stream, peer, cfg, pool, tls, start).await;
            });
        }
    }))
}

async fn handle_https_conn(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    cfg: Arc<Config>,
    pool: BackendPool,
    tls: TlsState,
    start: Instant,
) {
    let acceptor = LazyConfigAcceptor::new(Acceptor::default(), stream);
    let handshake = match acceptor.await {
        Ok(h) => h,
        Err(e) => {
            debug!("[proxy] tls preface from {peer}: {e}");
            return;
        }
    };

    let is_acme = handshake
        .client_hello()
        .alpn()
        .map(|mut iter| iter.any(|p| p == ACME_TLS_ALPN))
        .unwrap_or(false);

    if is_acme {
        let mut cfg_acme = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(alpn_resolver(tls.alpn_challenges.clone()));
        cfg_acme.alpn_protocols = vec![ACME_TLS_ALPN.to_vec()];
        let _ = handshake.into_stream(Arc::new(cfg_acme)).await;
        return;
    }

    let server_cfg = tls.config.load_full();
    let tls_stream = match handshake.into_stream(server_cfg).await {
        Ok(s) => s,
        Err(e) => {
            debug!("[proxy] tls handshake from {peer}: {e}");
            return;
        }
    };

    let io = TokioIo::new(tls_stream);
    let svc = service_fn(move |req| {
        let cfg = cfg.clone();
        let pool = pool.clone();
        async move {
            if req.uri().path() == cfg.health_path {
                return Ok::<_, hyper::Error>(status::handle(&pool, start).await);
            }
            Ok::<_, hyper::Error>(proxy_request(req, pool, peer, true).await)
        }
    });
    if let Err(e) = server_http1::Builder::new()
        .serve_connection(io, svc)
        .with_upgrades()
        .await
    {
        debug!("[proxy] https conn from {peer}: {e}");
    }
}

async fn proxy_request(
    mut req: Request<Incoming>,
    pool: BackendPool,
    peer: SocketAddr,
    https: bool,
) -> Response<ResponseBody> {
    let handle = match pool.pick() {
        Some(h) => h,
        None => {
            warn!("[proxy] no live backends");
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(full(b"no backends".to_vec()))
                .unwrap();
        }
    };

    inject_forwarded_headers(req.headers_mut(), peer, https);
    // Force the upstream to close after this request. We don't pool UDS connections
    // (handshake on loopback is microseconds); reuse would otherwise leak keep-alive
    // sockets on the Node side and balloon CPU/RAM until the worker freezes.
    req.headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));

    let backend_id = handle.backend.id;

    let stream = match tokio::time::timeout(
        UDS_CONNECT_TIMEOUT,
        UnixStream::connect(&handle.backend.socket_path),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            warn!("[proxy] uds connect backend {backend_id}: {e}");
            return bad_gateway("backend connect failed");
        }
        Err(_) => {
            warn!("[proxy] uds connect timeout backend {backend_id}");
            return bad_gateway("backend connect timeout");
        }
    };
    let io = TokioIo::new(stream);
    let (mut sender, conn) = match tokio::time::timeout(
        UDS_HANDSHAKE_TIMEOUT,
        client_http1::handshake(io),
    )
    .await
    {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            warn!("[proxy] uds handshake backend {backend_id}: {e}");
            return bad_gateway("backend handshake failed");
        }
        Err(_) => {
            warn!("[proxy] uds handshake timeout backend {backend_id}");
            return bad_gateway("backend handshake timeout");
        }
    };

    // Drive the connection task with a hard upper bound. Even with `Connection: close`
    // we don't want a buggy/wedged upstream to keep this task alive forever.
    let conn_handle = tokio::spawn(async move {
        let _ = tokio::time::timeout(UPSTREAM_CONN_DEADLINE, conn).await;
    });

    match tokio::time::timeout(UPSTREAM_REQUEST_TIMEOUT, sender.send_request(req)).await {
        Ok(Ok(res)) => {
            // Body streams through the conn task. Detach: the inflight handle keeps the
            // backend marked busy until this future is dropped — but since the body is
            // returned upstream, we can't hold it here. The conn task self-terminates
            // when the body is fully read (server FINs due to Connection: close) or via
            // the UPSTREAM_CONN_DEADLINE timeout above.
            drop(handle);
            // Hand off sender ownership so it lives until the body is consumed.
            // (sender drop signals the conn task; we let conn task own its own lifetime
            // via the deadline.)
            drop(sender);
            res.map(|b| b.boxed())
        }
        Ok(Err(e)) => {
            warn!("[proxy] backend {backend_id} send: {e}");
            conn_handle.abort();
            bad_gateway("backend request failed")
        }
        Err(_) => {
            warn!("[proxy] backend {backend_id} request timeout");
            conn_handle.abort();
            bad_gateway("backend request timeout")
        }
    }
}

fn bad_gateway(msg: &'static str) -> Response<ResponseBody> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(full(msg.as_bytes().to_vec()))
        .unwrap()
}

fn inject_forwarded_headers(headers: &mut hyper::HeaderMap, peer: SocketAddr, https: bool) {
    static XFF: HeaderName = HeaderName::from_static("x-forwarded-for");
    static XFP: HeaderName = HeaderName::from_static("x-forwarded-proto");
    static XRI: HeaderName = HeaderName::from_static("x-real-ip");

    let ip = peer.ip().to_string();
    let proto = if https { "https" } else { "http" };

    if let Some(existing) = headers.get(&XFF).and_then(|v| v.to_str().ok()) {
        let chained = format!("{existing}, {ip}");
        if let Ok(v) = HeaderValue::from_str(&chained) {
            headers.insert(&XFF, v);
        }
    } else if let Ok(v) = HeaderValue::from_str(&ip) {
        headers.insert(&XFF, v);
    }
    headers.insert(&XFP, HeaderValue::from_static(proto));
    if let Ok(v) = HeaderValue::from_str(&ip) {
        headers.insert(&XRI, v);
    }
}

fn empty() -> ResponseBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full(data: Vec<u8>) -> ResponseBody {
    Full::new(Bytes::from(data))
        .map_err(|never| match never {})
        .boxed()
}
