use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
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
pub(crate) struct ConnectInfoAcceptor<A> {
    pub inner: A,
    pub proxy_protocol: bool,
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
        Box::pin(async move {
            let mut stream = stream;
            let real_addr = if proxy_protocol {
                read_proxy_v1(&mut stream).await?
            } else {
                stream.peer_addr()?
            };
            let svc = ConnectInfoService { inner: service, addr: real_addr };
            inner.accept(stream, svc).await
        })
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
