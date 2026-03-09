use std::net::SocketAddr;

use warp::Filter;

use crate::proxy::handle_proxy;
use crate::server::cors;

pub async fn start() -> u16 {
    let http_client = reqwest::Client::new();

    let route = warp::path("p")
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
        )
        .with(cors());

    let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
    let (addr, server) = warp::serve(route).bind_ephemeral(addr);
    tokio::spawn(server);

    println!("[ProxyServer] http://127.0.0.1:{}", addr.port());
    addr.port()
}
