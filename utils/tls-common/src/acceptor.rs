use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};

use axum::extract::ConnectInfo;
use axum::http::Request;
use axum_server::accept::Accept;
use tokio::net::TcpStream;
use tower::Service;

use crate::proxy::read_proxy_v1;

/// Wraps inner `Accept` (как `rustls_acme::axum::AxumAcceptor`):
/// 1) при `proxy_protocol=true` читает PROXY v1 header → real client addr;
///    иначе берёт `tcp.peer_addr()`.
/// 2) оборачивает service в `ConnectInfoService` чтобы каждый Request получил
///    `ConnectInfo<SocketAddr>` extension.
#[derive(Clone)]
pub(crate) struct TrustedProxies {
    pub cidrs: Arc<Vec<crate::config::IpCidr>>,
    pub resolved: Arc<RwLock<Vec<IpAddr>>>,
}

impl TrustedProxies {
    pub fn contains(&self, ip: IpAddr) -> bool {
        if self.cidrs.iter().any(|c| c.contains(ip)) {
            return true;
        }
        self.resolved
            .read()
            .map(|r| r.contains(&ip))
            .unwrap_or(false)
    }
}

#[derive(Clone)]
pub(crate) struct ConnectInfoAcceptor<A> {
    pub inner: A,
    pub proxy_protocol: bool,
    pub proxy_trusted: TrustedProxies,
}

impl<A, S> Accept<TcpStream, S> for ConnectInfoAcceptor<A>
where
    A: Accept<TcpStream, ConnectInfoService<S>> + Clone + Send + Sync + 'static,
    A::Future: Send + 'static,
    A::Stream: Send + 'static,
    A::Service: Send + 'static,
    S: Send + 'static,
{
    type Stream = A::Stream;
    type Service = A::Service;
    type Future = Pin<Box<dyn Future<Output = io::Result<(Self::Stream, Self::Service)>> + Send>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let inner = self.inner.clone();
        let proxy_protocol = self.proxy_protocol;
        let proxy_trusted = self.proxy_trusted.clone();
        Box::pin(async move {
            let mut stream = stream;
            let peer = stream.peer_addr()?;
            // PROXY header is consumed when present (else its bytes corrupt TLS)
            // but only trusted from allowlisted peers; otherwise use peer_addr so
            // a direct connector can't spoof source-IP by forging it.
            let real_addr = if proxy_protocol && peek_proxy_signature(&stream).await {
                let advertised = read_proxy_v1(&mut stream).await?;
                if proxy_trusted.contains(peer.ip()) {
                    advertised
                } else {
                    peer
                }
            } else {
                peer
            };
            let svc = ConnectInfoService {
                inner: service,
                addr: real_addr,
            };
            inner.accept(stream, svc).await
        })
    }
}

async fn peek_proxy_signature(stream: &TcpStream) -> bool {
    let mut sig = [0u8; 6];
    match stream.peek(&mut sig).await {
        Ok(n) if n >= 1 => {
            let want = b"PROXY ";
            sig[..n] == want[..n.min(want.len())]
        }
        _ => false,
    }
}

/// Per-connection wrapper, добавляющий `ConnectInfo<SocketAddr>` в request extensions.
/// Аналог axum'овского `into_make_service_with_connect_info::<SocketAddr>()`, но с
/// addr полученным сверху (PROXY или peer_addr) а не TCP socket'а.
#[derive(Clone)]
pub(crate) struct ConnectInfoService<S> {
    pub inner: S,
    pub addr: SocketAddr,
}

impl<S, B> Service<Request<B>> for ConnectInfoService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        req.extensions_mut().insert(ConnectInfo(self.addr));
        self.inner.call(req)
    }
}
